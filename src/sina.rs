// todo source provider

use std::{str::FromStr, time::Duration};

use anyhow::{bail, Result};
use futures_util::{SinkExt, StreamExt};
use http::{Method, Request};
use regex::Regex;
use reqwest::{
    header::{self, HeaderMap, HeaderValue},
    Client, StatusCode,
};
use scraper::{Html, Selector};
use tokio::{select, time::interval};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::debug;

use crate::invest::{
    quote::Quote,
    stock::{Financial, Profile},
    Exchange, Investment, Market,
};

const PORTAL: &str = "https://finance.sina.com.cn";

#[derive(Debug)]
pub struct Sina {
    client: Client,
}

impl Default for Sina {
    fn default() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(header::REFERER, HeaderValue::from_static(PORTAL));
        let client = Client::builder().default_headers(headers).timeout(Duration::from_secs(5)).build().unwrap();
        Sina { client }
    }
}

impl Sina {
    pub fn new(client: Client) -> Self {
        Sina { client }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Investment>> {
        // todo config or args
        // type 沪深 11,12,13,14,15  基金 21,22,23,24,25,26  港股 31,32,33 美股 41,42
        match self
            .request(&format!(
                "https://suggest3.sinajs.cn/suggest/type=11,12,13,14,15,21,22,23,24,25,26,31&key={}",
                query
            ))
            .await
        {
            Ok(content) => {
                if let Some(caps) = Regex::new("\"(.*)\"").unwrap().captures(&content) {
                    // 腾讯控股,31,00700,00700,腾讯控股,,腾讯控股,99,1,ESG;
                    // 1 5 7名称 2市场 3 4代码 8- 9在市 10-
                    let mut values = Vec::new();
                    let pieces = caps.get(1).unwrap().as_str().split(';').collect::<Vec<&str>>();
                    for p in pieces.iter() {
                        values.push(p.split(',').collect::<Vec<&str>>());
                    }

                    let mut investments = Vec::new();
                    for v in values.iter() {
                        if v.get(8).unwrap() == &"1" {
                            let mut symbol = v.get(3).unwrap().to_uppercase();
                            let mut market = None;
                            let mut exchange = None;
                            match *v.get(1).unwrap() {
                                "11" | "12" | "13" | "14" | "15" => {
                                    market = Some(Market::Stock);
                                    exchange = match Exchange::from_str(&symbol) {
                                        Ok(ex) => Some(ex),
                                        Err(_) => None,
                                    }
                                }
                                "21" | "22" | "23" | "24" | "25" | "26" => {
                                    market = Some(Market::Fund);
                                }
                                "31" => {
                                    market = Some(Market::Stock);
                                    exchange = Some(Exchange::HKex);
                                    symbol = "HK".to_owned() + &symbol;
                                }
                                _ => {}
                            }

                            investments.push(Investment {
                                code: v.get(2).unwrap().to_string(),
                                symbol,
                                name: v.get(4).unwrap().to_string(),
                                market,
                                exchange,
                            })
                        }
                    }

                    return Ok(investments);
                }

                Ok(Vec::new())
            }
            Err(err) => bail!(err),
        }
    }

    pub async fn profile(&self, code: &str) -> Result<Profile> {
        match self
            .request(&format!("https://vip.stock.finance.sina.com.cn/corp/go.php/vCI_CorpInfo/stockid/{}.phtml", code))
            .await
        {
            Ok(content) => {
                let doc = Html::parse_document(&content);
                let tds = Selector::parse("#comInfo1 td").unwrap();
                let a = Selector::parse("a").unwrap();
                let mut profile = Profile::default();
                for (i, td) in doc.select(&tds).into_iter().enumerate() {
                    match i {
                        1 => profile.name = td.inner_html().trim().to_string(),
                        7 => profile.listing_date = td.select(&a).next().unwrap().inner_html().trim().to_string(),
                        9 => profile.listing_price = td.inner_html().trim().to_string(),
                        35 => profile.website = td.select(&a).next().unwrap().inner_html().trim().to_string(),
                        41 => profile.used_name = td.inner_html().trim().to_string(),
                        45 => profile.business_address = td.inner_html().trim().to_string(),
                        49 => profile.business = td.inner_html().trim().to_string(),
                        _ => {}
                    }
                }

                Ok(profile)
            }
            Err(err) => bail!(err),
        }
    }

    pub async fn financials(&self, code: &str) -> Result<Vec<Financial>> {
        match self
            .request(&format!(
                "https://money.finance.sina.com.cn/corp/go.php/vFD_FinanceSummary/stockid/{}.phtml",
                code
            ))
            .await
        {
            Ok(content) => {
                let doc = Html::parse_document(&content);
                // todo

                Ok(Vec::new())
            }
            Err(err) => bail!(err),
        }
    }

    pub async fn quote(&self, symbol: &str) -> Result<Quote> {
        match self.request(&format!("https://hq.sinajs.cn/list={}", symbol.to_lowercase())).await {
            Ok(content) => {
                if let Some(caps) = Regex::new("\"(.*)\"").unwrap().captures(&content) {
                    let mut quote = Quote::from(caps.get(1).unwrap().as_str());
                    quote.symbol = symbol.to_string();

                    return Ok(quote);
                }

                Ok(Quote::default())
            }
            Err(err) => bail!(err),
        }
    }

    async fn request(&self, url: &str) -> Result<String> {
        match self.client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                let content = resp.text().await.unwrap();
                if status != StatusCode::OK {
                    bail!("request return error, http code: {}, content: {}", status, &content)
                }

                Ok(content)
            }
            Err(err) => bail!("request failed, {}", err),
        }
    }

    pub async fn quote_ws(symbol: &str, handler: impl Fn(Quote)) {
        let req = Request::builder()
            .method(Method::GET)
            .uri(format!("wss://hq.sinajs.cn/wskt?list={}", symbol.to_lowercase()))
            .header(header::ORIGIN, HeaderValue::from_static(PORTAL))
            .body(())
            .unwrap();

        let (ws, _) = connect_async(req).await.unwrap();
        let (mut sender, mut receiver) = ws.split();
        let mut interval = interval(Duration::from_secs(60));

        loop {
            select! {
                msg = receiver.next() => {
                    if let Some(msg) = msg {
                        let msg = msg.unwrap();
                        if msg.is_text() {
                            debug!("ws receive msg: {}", msg);
                            if let Some(caps) = Regex::new("=(.*)\\n").unwrap().captures(&msg.to_string()) {
                                let mut quote = Quote::from(caps.get(1).unwrap().as_str());
                                quote.symbol = symbol.to_string();
                                handler(quote);
                            }
                        }
                    }
                }
                _ = interval.tick() => {
                    sender.send(Message::Text("".to_string())).await.unwrap();
                }
            }
        }
    }
}
