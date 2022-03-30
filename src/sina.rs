// todo source provider

use std::{f64, str::FromStr, time::Duration};

use anyhow::{bail, Result};
use futures_util::{SinkExt, StreamExt};
use http::{Method, Request};
use regex::Regex;
use reqwest::{
    header::{self, HeaderMap, HeaderValue},
    Client, StatusCode,
};
use scraper::{ElementRef, Html, Node, Selector};
use tokio::{join, select, time::interval};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error};

use crate::invest::{
    quote::Quote,
    stock::{Dividend, Financial, Holder, Press, Profile, Structure},
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
        let client = Client::builder().default_headers(headers).timeout(Duration::from_secs(10)).build().unwrap();
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
                "https://suggest3.sinajs.cn/suggest/type=11,12,13,14,15,21,22,23,24,25,26,31,41&key={}",
                query
            ))
            .await
        {
            Ok(content) => {
                debug!("search result: {}", content);
                if let Some(caps) = Regex::new("\"(.*)\"").unwrap().captures(&content) {
                    // 腾讯控股,31,00700,00700,腾讯控股,,腾讯控股,99,1,ESG;
                    // 1 5 7名称 2市场 3 4代码 8- 9在市 10-
                    let matched = caps.get(1).unwrap().as_str();
                    if matched.is_empty() {
                        return Ok(Vec::new());
                    }

                    let mut values = Vec::new();
                    let pieces = matched.split(';').collect::<Vec<&str>>();
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
                                    exchange = match Exchange::from_str(&symbol[..2]) {
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
                                "41" | "42" => {
                                    market = Some(Market::Stock);
                                    // todo exchange check
                                    // exchange = Some(Exchange::Nyse);
                                    // symbol = "US".to_owned() + &symbol;
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

    pub async fn profile(&self, symbol: &str) -> Result<Profile> {
        let corp_url =
            format!("https://vip.stock.finance.sina.com.cn/corp/go.php/vCI_CorpInfo/stockid/{}.phtml", &symbol[2..]);
        let info_url = format!("https://hq.sinajs.cn/list={},{}_i", symbol.to_lowercase(), symbol.to_lowercase());
        let (corp, info) = join!(self.request(&corp_url), self.request(&info_url));

        let mut profile = Profile::default();
        match corp {
            Ok(content) => {
                let doc = Html::parse_document(&content);
                let tds = Selector::parse("#comInfo1 td").unwrap();
                let a = Selector::parse("a").unwrap();
                for (i, td) in doc.select(&tds).enumerate() {
                    match i {
                        1 => profile.name = td.inner_html().trim().to_string(),
                        7 => profile.listing_date = td.select(&a).next().unwrap().inner_html().trim().to_string(),
                        9 => profile.listing_price = td.inner_html().trim().parse::<f64>().unwrap_or(0.0),
                        35 => profile.website = td.select(&a).next().unwrap().inner_html().trim().to_string(),
                        41 => profile.used_name = td.inner_html().trim().to_string(),
                        45 => profile.business_address = td.inner_html().trim().to_string(),
                        49 => profile.business = td.inner_html().trim().to_string(),
                        _ => {}
                    }
                }
            }
            Err(err) => error!("get corp failed, {}", err),
        }

        match info {
            Ok(content) => {
                for (i, caps) in Regex::new("\"(.*)\"").unwrap().captures_iter(&content).enumerate() {
                    match i {
                        0 => {
                            let quote = quote_from_str(caps.get(1).unwrap().as_str());
                            profile.price = quote.now;
                            if profile.used_name.is_empty() {
                                profile.used_name = quote.name;
                            }
                        }
                        1 => {
                            // A,zgpa,8.1000,6.6573,4.6300,43.3277,2859.1461,1828024.141,1083266.4498,1083266.4498,0,CNY,1430.9900,1216.9600,33.8000,1,10.5000,9046.2900,816.3800,88.280,47.300,0.1,中国平安,X|O|0|0|0,55.87|45.71,20210930|27212666666.67,637.4600|81.8240,|,,1/1,EQA,,1.61,50.41|50.41|53.55,保险Ⅱ
                            let info = caps.get(1).unwrap().as_str().split(',').collect::<Vec<&str>>();
                            let eps = info.get(5).unwrap_or(&"").parse::<f64>().unwrap_or(0.0);
                            let cap = info.get(7).unwrap_or(&"").parse::<f64>().unwrap_or(0.0);
                            let traded_cap = info.get(8).unwrap_or(&"").parse::<f64>().unwrap_or(0.0);

                            profile.pb = profile.price / eps;
                            profile.pb = if profile.pb.is_nan() { 0.0 } else { profile.pb };
                            profile.category = info.get(34).unwrap_or(&"").to_string();
                            profile.market_cap = profile.price * cap * 10000.0;
                            profile.traded_market_cap = profile.price * traded_cap * 10000.0;
                            // todo calc profile.pe_ttm
                        }
                        _ => {}
                    }
                }
            }
            Err(err) => error!("get info failed, {}", err),
        }

        Ok(profile)
    }

    // todo complete info
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
                let tds = Selector::parse("#FundHoldSharesTable tr td:last-child").unwrap();
                let val = Selector::parse("strong,a").unwrap();
                let mut results = Vec::new();
                let mut financials = Vec::new();
                let mut f = Financial::default();
                let to_num = |s: &str| s.replace(',', "").replace('元', "").parse::<f64>().unwrap_or(0.0);
                for (i, td) in doc.select(&tds).enumerate() {
                    let val = match td.first_child() {
                        Some(node) => match node.value() {
                            Node::Text(txt) => txt.text.to_string(),
                            Node::Element(_) => td.select(&val).next().unwrap().inner_html(),
                            _ => "".to_string(),
                        },
                        None => "".to_string(),
                    };

                    match i {
                        _ if i % 12 == 0 => {
                            if i > 0 {
                                financials.push(f);
                                f = Financial::default();
                            }
                            f.date = val;
                        }
                        _ if i % 12 == 1 => f.ps_net_assets = to_num(&val),
                        _ if i % 12 == 3 => f.ps_capital_reserve = to_num(&val),
                        _ if i % 12 == 8 => f.total_revenue = to_num(&val),
                        _ if i % 12 == 10 => f.net_profit = to_num(&val),
                        _ if i > 95 => break, // 取最近8季度
                        _ => {}
                    }
                }

                for i in 0..4 {
                    if let Some(cur) = financials.get(i) {
                        let mut f = cur.clone();
                        if let Some(prev) = financials.get(i + 4) {
                            f.total_revenue_rate = (f.total_revenue - prev.total_revenue) / prev.total_revenue * 100.0;
                            f.net_profit_rate = (f.net_profit - prev.net_profit) / prev.net_profit * 100.0;
                        }
                        results.push(f);
                    }
                }

                Ok(results)
            }
            Err(err) => bail!("get financials failed, {}", err),
        }
    }

    pub async fn structures(&self, code: &str) -> Result<Vec<Structure>> {
        match self
            .request(&format!(
                "https://vip.stock.finance.sina.com.cn/corp/go.php/vCI_StockHolder/stockid/{}.phtml",
                code
            ))
            .await
        {
            Ok(content) => {
                let doc = Html::parse_document(&content);
                let trs = Selector::parse("#Table1 tbody tr").unwrap();
                let td1 = Selector::parse("td:last-child").unwrap();
                let td2 = Selector::parse("td div").unwrap();
                let mut structures = Vec::new();
                let mut s = Structure::default();
                let get_link_val = |er: ElementRef| -> String {
                    let a = Selector::parse("a").unwrap();
                    match er.first_child().unwrap().value() {
                        Node::Text(txt) => txt.text.to_string(),
                        Node::Element(_) => er.select(&a).next().unwrap().inner_html(),
                        _ => "".to_string(),
                    }
                };
                for (i, tr) in doc.select(&trs).enumerate() {
                    let val = tr.select(&td1).next().unwrap().inner_html();

                    match i {
                        _ if i % 17 == 0 => {
                            if i > 0 {
                                structures.push(s);
                                s = Structure::default();
                            }
                            s.date = val;
                        }
                        _ if i % 17 == 3 => s.holders_num = num_from_str(&val),
                        _ if i % 17 == 4 => s.shares_avg = num_from_str(&val),
                        _ if i % 17 == 6
                            || i % 17 == 7
                            || i % 17 == 8
                            || i % 17 == 9
                            || i % 17 == 10
                            || i % 17 == 11
                            || i % 17 == 12
                            || i % 17 == 13
                            || i % 17 == 14
                            || i % 17 == 15 =>
                        {
                            let mut h = Holder::default();
                            for (m, td) in tr.select(&td2).enumerate() {
                                match m {
                                    1 => h.name = get_link_val(td),
                                    2 => h.shares = get_link_val(td).parse::<f64>().unwrap_or(0.0),
                                    3 => h.percent = get_link_val(td).parse::<f64>().unwrap_or(0.0),
                                    4 => h.shares_type = get_link_val(td),
                                    _ => {}
                                }
                            }
                            s.holders_ten.push(h);
                        }
                        _ => {}
                    }

                    if i > 68 {
                        break;
                    }
                }

                Ok(structures)
            }
            Err(err) => bail!("get presses failed, {}", err),
        }
    }

    pub async fn dividends(&self, code: &str) -> Result<Vec<Dividend>> {
        match self
            .request(&format!(
                "https://vip.stock.finance.sina.com.cn/corp/go.php/vISSUE_ShareBonus/stockid/{}.phtml",
                code
            ))
            .await
        {
            Ok(content) => {
                let doc = Html::parse_document(&content);
                let tds = Selector::parse("#sharebonus_1 tr td").unwrap();
                let mut dividends = Vec::new();
                let mut d = Dividend::default();
                for (i, td) in doc.select(&tds).enumerate() {
                    let val = td.inner_html();
                    match i {
                        _ if i % 9 == 0 => {
                            if i > 0 {
                                dividends.push(d);
                                d = Dividend::default();
                            }
                            d.date = val;
                        }
                        _ if i % 9 == 1 => d.shares_dividend = val.parse::<f64>().unwrap_or(0.0),
                        _ if i % 9 == 2 => d.shares_into = val.parse::<f64>().unwrap_or(0.0),
                        _ if i % 9 == 3 => d.money = val.parse::<f64>().unwrap_or(0.0),
                        _ if i % 9 == 5 => d.date_dividend = val,
                        _ if i % 9 == 6 => d.date_record = val,
                        _ => {}
                    }
                }

                Ok(dividends)
            }
            Err(err) => bail!("get dividends failed, {}", err),
        }
    }

    pub async fn presses(&self, code: &str) -> Result<Vec<Press>> {
        match self
            .request(&format!(
                "https://vip.stock.finance.sina.com.cn/corp/go.php/vCB_AllBulletin/stockid/{}.phtml",
                code
            ))
            .await
        {
            Ok(content) => {
                let doc = Html::parse_document(&content);
                let ul = Selector::parse("div.datelist ul").unwrap();
                let mut presses = Vec::new();
                let mut p = Press::default();
                if let Some(ele) = doc.select(&ul).next() {
                    for (i, item) in ele.children().enumerate() {
                        match i {
                            _ if i % 3 == 0 => {
                                if i > 0 {
                                    presses.push(p);
                                    p = Press::default();
                                }
                                p.date = item.value().as_text().unwrap().trim().to_string();
                            }
                            _ if i % 3 == 1 => {
                                let ele = item.value().as_element().unwrap();
                                p.url = format!("https://vip.stock.finance.sina.com.cn/{}", ele.attr("href").unwrap());
                                let txt = item.children().next().unwrap().value().as_text().unwrap();
                                p.title = txt.text.to_string();
                            }
                            _ => {}
                        }
                    }
                }

                Ok(presses)
            }
            Err(err) => bail!("get presses failed, {}", err),
        }
    }

    /// symbols: sz000001,sh601318
    pub async fn quotes(&self, symbols: &str) -> Result<Vec<Quote>> {
        match self.request(&format!("https://hq.sinajs.cn/list={}", fmt_quote_symbols(symbols))).await {
            Ok(content) => {
                debug!("quotes result: {}", content);
                let quotes = quotes_from_str("hq_str_(?:rt_)?([a-z0-9]+)=\"(.*)\"", &content);
                Ok(quotes)
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

    /// 多个时连接时返回所有 之后单个返回
    pub async fn quotes_ws(symbols: &str, handler: impl Fn(Vec<Quote>)) {
        let req = Request::builder()
            .method(Method::GET)
            .uri(format!("wss://hq.sinajs.cn/wskt?list={}", fmt_quote_symbols(symbols)))
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
                            let quotes = quotes_from_str("(?:rt_)?([a-z0-9]+)=(.*)\\n", &msg.to_string());
                            handler(quotes);
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

// hk 代码格式 rt_hk00700 获取实时行情
fn fmt_quote_symbols(symbols: &str) -> String {
    symbols.to_lowercase().replace("hk", "rt_hk")
}

// 中国平安,51.020,50.790,49.970,51.350,49.800,49.970,49.980,72935539,3688023391.000,155984,49.970,125200,49.960,95800,49.950,48800,49.940,32300,49.930,174297,49.980,10800,49.990,86300,50.000,3100,50.010,53700,50.020,2022-01-28,15:00:00,00,
fn quote_from_str(str: &str) -> Quote {
    let values: Vec<&str> = str.split(',').collect::<Vec<&str>>();
    Quote {
        symbol: "".to_string(),
        name: values.get(0).unwrap_or(&"").to_string(),
        now: values.get(3).unwrap_or(&"").parse().unwrap_or(0.0),
        close: values.get(2).unwrap_or(&"").parse().unwrap_or(0.0),
        open: values.get(1).unwrap_or(&"").parse().unwrap_or(0.0),
        high: values.get(4).unwrap_or(&"").parse().unwrap_or(0.0),
        low: values.get(5).unwrap_or(&"").parse().unwrap_or(0.0),
        buy: values.get(6).unwrap_or(&"").parse().unwrap_or(0.0),
        sell: values.get(7).unwrap_or(&"").parse().unwrap_or(0.0),
        turnover: values.get(8).unwrap_or(&"").parse().unwrap_or(0.0),
        volume: values.get(9).unwrap_or(&"").parse().unwrap_or(0.0),
        date: values.get(30).unwrap_or(&"").to_string(),
        time: values.get(31).unwrap_or(&"").to_string(),
    }
}

// TENCENT,腾讯控股,371.000,366.400,380.400,370.000,377.200,10.800,2.948,377.00000,377.20001,7860991814,20901992,0.000,0.000,658.000,297.000,2022/03/29,16:00
fn quote_from_str_hk(str: &str) -> Quote {
    let values: Vec<&str> = str.split(',').collect::<Vec<&str>>();
    Quote {
        symbol: "".to_string(),
        name: values.get(1).unwrap_or(&"").to_string(),
        now: values.get(6).unwrap_or(&"").parse().unwrap_or(0.0),
        close: values.get(3).unwrap_or(&"").parse().unwrap_or(0.0),
        open: values.get(2).unwrap_or(&"").parse().unwrap_or(0.0),
        high: values.get(4).unwrap_or(&"").parse().unwrap_or(0.0),
        low: values.get(5).unwrap_or(&"").parse().unwrap_or(0.0),
        buy: values.get(6).unwrap_or(&"").parse().unwrap_or(0.0),
        sell: values.get(7).unwrap_or(&"").parse().unwrap_or(0.0),
        turnover: values.get(12).unwrap_or(&"").parse().unwrap_or(0.0),
        volume: values.get(11).unwrap_or(&"").parse().unwrap_or(0.0),
        date: values.get(17).unwrap_or(&"").replace('/', "-"),
        time: values.get(18).unwrap_or(&"").to_string(),
    }
}

fn quotes_from_str(regex: &str, str: &str) -> Vec<Quote> {
    let mut quotes = Vec::new();
    let regex = Regex::new(regex).unwrap();
    for caps in regex.captures_iter(str) {
        let quote_str = caps.get(2).unwrap().as_str();
        let symbol = caps.get(1).unwrap().as_str();
        let mut quote = match Exchange::from_str(&symbol[..2]) {
            Ok(ex) => match ex {
                Exchange::Sse | Exchange::SZse | Exchange::Bse => quote_from_str(quote_str),
                Exchange::HKex => quote_from_str_hk(quote_str),
                _ => continue,
            },
            Err(err) => {
                debug!("{}", err);
                continue;
            }
        };

        quote.symbol = symbol.to_uppercase();
        quotes.push(quote);
    }

    quotes
}

fn num_from_str(str: &str) -> f64 {
    match Regex::new(r"\d+").unwrap().captures(str) {
        Some(caps) => caps.get(0).unwrap().as_str().parse::<f64>().unwrap_or(0.0),
        None => 0.0,
    }
}
