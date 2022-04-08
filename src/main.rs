use std::{
    collections::HashMap,
    io::{stdout, Write},
    str::FromStr,
};

use anyhow::{bail, Result};
use crossterm::{cursor, style::Stylize, terminal, terminal::ClearType, ExecutableCommand};
use once_cell::sync::Lazy;
use owo_colors::OwoColorize;
use rains::{
    cli::{Opts, Subcommand},
    invest::{quote::Quote, Exchange, Investment, Market},
    sina::Sina,
};
use tokio::sync::Mutex;
use tracing::{debug, error};
use tracing_subscriber::EnvFilter;

static SINA: Lazy<Mutex<Sina>> = Lazy::new(|| Mutex::new(Sina::default()));

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        error!("{}", err);
        std::process::exit(0x0100)
    }
}

// todo use tui table & chart

#[allow(clippy::mutex_atomic)]
async fn run() -> Result<()> {
    let args = Opts::parse_args()?;
    if args.debug {
        let filter = "debug,html5ever=info,selectors=info".parse::<EnvFilter>().unwrap();
        tracing_subscriber::fmt().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().without_time().with_target(false).init();
    }
    debug!("args: {:?}", args);

    match args.cmd {
        Subcommand::Search { query, limit } => match Sina::default().search(&query).await {
            Ok(results) => {
                let limit = if (limit as usize) < results.len() { limit as usize } else { results.len() };
                for i in 0..limit {
                    let invest = results.get(i).unwrap();
                    println!("{:<8}\t{}", invest.symbol, invest.name);
                }
            }
            Err(err) => error!("{}", err),
        },
        Subcommand::Info { symbol, all, financials, structure, dividends, presses } => {
            match check_symbol(&symbol).await {
                Ok(invest) => {
                    match invest.exchange {
                        Some(Exchange::Sse) | Some(Exchange::SZse) | Some(Exchange::Bse) => {}
                        _ => bail!("当前仅支持沪深北证股票信息查询"),
                    }

                    let symbol = invest.symbol.clone();
                    match SINA.lock().await.profile(&symbol).await {
                    Ok(profile) => println!(
                        "{}\n证券代码\t{}\n简称历史\t{}\n公司名称\t{}\n上市日期\t{}\n发行价格\t{:.2}\n行业分类\t{}\n主营业务\t{}\n办公地址\t{}\n公司网址\t{}\n当前价格\t{:.2}\n市净率PB\t{:.2}\n市盈率TTM\t{}\n总市值  \t{}\n流通市值\t{}",
                        "基本信息".bold(),
                        &symbol,
                        profile.used_name,
                        profile.name,
                        profile.listing_date,
                        profile.listing_price,
                        profile.category,
                        profile.business,
                        profile.business_address,
                        profile.website.underline(),
                        profile.price,
                        profile.pb,
                        fmt_num(&profile.pe_ttm),
                        fmt_num(&profile.market_cap),
                        fmt_num(&profile.traded_market_cap)
                    ),
                    Err(err) => error!("{}", err),
                }

                    if all || financials {
                        println!("\n{}", "财务指标".bold());
                        let symbol = symbol.clone();
                        tokio::spawn(async move {
                            match SINA.lock().await.financials(&symbol[2..]).await {
                                Ok(financials) => {
                                    // align todo change
                                    let cols = vec!["截止日期", "总营收", "净利润", "每股净资产", "每股资本公积金"];
                                    for (i, col) in cols.iter().enumerate() {
                                        let mut output = format!("{:<16}", *col);
                                        for f in financials.iter() {
                                            match i {
                                                0 => output.push_str(&format!("\t{:<16}", f.date)),
                                                1 => output.push_str(&format!("\t{:<16}", fmt_num(&f.total_revenue))),
                                                2 => output.push_str(&format!("\t{:<16}", fmt_num(&f.net_profit))),
                                                3 => output
                                                    .push_str(&format!("\t{:<16}", format!("{:.4}", f.ps_net_assets))),
                                                4 => output.push_str(&format!(
                                                    "\t{:<16}",
                                                    format!("{:.4}", f.ps_capital_reserve)
                                                )),
                                                _ => {}
                                            }
                                        }
                                        println!("{}", output);
                                    }
                                }
                                Err(err) => error!("{}", err),
                            }
                        })
                        .await
                        .unwrap();
                    }

                    if all || structure {
                        println!("\n{}", "股东结构".bold());
                        let symbol = invest.symbol.clone();
                        tokio::spawn(async move {
                            match SINA.lock().await.structures(&symbol[2..]).await {
                                Ok(structures) => {
                                    if structures.is_empty() {
                                        return;
                                    }

                                    let first = structures.get(0).unwrap();
                                    let mut holders = String::new();
                                    let mut shares = String::new();
                                    for s in structures.iter() {
                                        holders.push_str(&format!("{}({})\t", fmt_num(&s.holders_num), s.date));
                                        shares.push_str(&format!("{}({})\t", fmt_num(&s.shares_avg), s.date));
                                    }
                                    println!(
                                        "截止日期\t{}\n股东户数\t{}\n平均持股\t{}\n十大股东",
                                        first.date, holders, shares
                                    );
                                    for (i, h) in first.holders_ten.iter().enumerate() {
                                        println!("{}\t{}({}% {})", i + 1, h.name, h.percent, fmt_num(&h.shares))
                                    }
                                }
                                Err(err) => error!("{}", err),
                            }
                        })
                        .await
                        .unwrap()
                    }

                    if all || dividends {
                        println!("\n{}", "分红送配".bold());
                        let symbol = invest.symbol.clone();
                        tokio::spawn(async move {
                            match SINA.lock().await.dividends(&symbol[2..]).await {
                                Ok(dividends) => {
                                    println!("公告日期 \t 分红送配 \t\t\t 除权除息日 \t 股权登记日");
                                    for d in dividends.iter() {
                                        let mut info = String::from("10");
                                        if d.shares_dividend > 0.0 {
                                            info.push_str(&format!("送{}股", d.shares_dividend));
                                        }
                                        if d.shares_into > 0.0 {
                                            info.push_str(&format!("转{}股", d.shares_into));
                                        }
                                        if d.money > 0.0 {
                                            info.push_str(&format!("派{}元", d.money));
                                        }
                                        if info.len() < 3 {
                                            info = String::from("不分配\t");
                                        }
                                        println!(
                                            "{} \t {} \t\t {} \t {}",
                                            d.date,
                                            if info.len() < 19 { format!("{}\t", info) } else { info },
                                            if d.date_dividend.len() < 3 { " -\t" } else { &d.date_dividend },
                                            if d.date_record.len() < 3 { " - " } else { &d.date_record }
                                        );
                                    }
                                }
                                Err(err) => error!("{}", err),
                            }
                        })
                        .await
                        .unwrap()
                    }

                    if all || presses {
                        println!("\n{}", "最新公告".bold());
                        let symbol = invest.symbol.clone();
                        tokio::spawn(async move {
                            match SINA.lock().await.presses(&symbol[2..]).await {
                                Ok(presses) => {
                                    for p in presses.iter() {
                                        println!("{}\t{}\t{}", p.date, p.title, p.url);
                                    }
                                }
                                Err(err) => error!("{}", err),
                            }
                        })
                        .await
                        .unwrap();
                    }
                }
                Err(err) => error!("{}", err),
            }
        }
        Subcommand::Quote { symbol, no_check, realtime, multiline } => {
            let parts = symbol.split(',').collect::<Vec<&str>>();
            let mut symbols = Vec::new();
            for symbol in parts {
                if no_check {
                    symbols.push(symbol.to_string());
                } else {
                    match check_symbol(symbol).await {
                        Ok(invest) => symbols.push(invest.symbol),
                        Err(err) => error!("{} {}", symbol, err),
                    }
                }
            }

            let symbols = symbols.join(",");
            if realtime {
                // 首次输出
                let written = std::sync::Mutex::new(false);
                // 实际数量
                let len = std::sync::Mutex::new(0);
                // 位置对应表
                let lines = std::sync::Mutex::new(HashMap::new());
                // 当前位置
                let cur = std::sync::Mutex::new(0);
                Sina::quotes_ws(&symbols, |quotes: Vec<Quote>| {
                    let mut l = len.lock().unwrap();
                    if multiline && quotes.len() == 1 && *l == 0 {
                        write_quote(quotes.get(0).unwrap());
                        return;
                    }

                    let mut stdout = stdout();
                    let mut w = written.lock().unwrap();
                    let mut m = lines.lock().unwrap();
                    let mut c = cur.lock().unwrap();
                    if !*w {
                        *l = quotes.len();
                        *c = quotes.len() - 1;
                    }
                    for (i, quote) in quotes.iter().enumerate() {
                        let k = quote.symbol.to_string();
                        if *w {
                            if let Some(line) = m.get(&k) {
                                if *c >= *line {
                                    stdout.execute(cursor::MoveToPreviousLine((*c - *line + 1) as u16)).unwrap();
                                } else if *line > *c + 1 {
                                    stdout.execute(cursor::MoveToNextLine((*line - *c - 1) as u16)).unwrap();
                                }
                                *c = *line;
                            }
                        } else {
                            m.insert(k, i);
                        }
                        stdout.execute(terminal::Clear(ClearType::CurrentLine)).unwrap();
                        write_quote(quote);
                    }
                    stdout.flush().unwrap();
                    *w = true;
                })
                .await;
            } else {
                match SINA.lock().await.quotes(&symbols).await {
                    Ok(quotes) => {
                        for quote in quotes.iter() {
                            write_quote(quote);
                        }
                    }
                    Err(err) => error!("{}", err),
                }
            }
        }
    }

    Ok(())
}

async fn check_symbol(symbol: &str) -> Result<Investment> {
    let invest = match Investment::from_str(symbol) {
        Ok(invest) => invest,
        Err(err) => bail!(err),
    };
    let mut query = invest.symbol.as_str();
    if query.starts_with("HK") {
        query = &query[2..];
    }

    match SINA.lock().await.search(query).await {
        Ok(res) => {
            if res.is_empty() {
                bail!("代码错误")
            }
            let first = res.first().unwrap();
            if first.market.is_none() || first.market != Some(Market::Stock) {
                bail!("代码错误")
            }
            Ok(first.clone())
        }
        Err(err) => bail!(err),
    }
}

fn fmt_num(num: &f64) -> String {
    match num {
        _ if *num > 100_000_000.0 => {
            format!("{:.2}亿", num / 100_000_000.0)
        }
        _ if *num == 0.0 => " - ".to_string(),
        _ => {
            format!("{:.2}万", num / 10_000.0)
        }
    }
}

fn write_quote(quote: &Quote) {
    let rate = (quote.now / quote.close - 1.0) * 100.0;
    let now = format!("{:.2} {:.2}%", quote.now, rate);
    println!(
        "{} {}  {:<16} \t昨收：{:.2}\t今开：{:.2}\t最高：{:.2}\t最低：{:.2}\t成交量：{}手\t成交额：{}元\t{}",
        quote.date,
        quote.time,
        match rate {
            _ if rate > 0.0 => now.red(),
            _ if rate < 0.0 => now.green(),
            _ => now.dark_grey(),
        }
        .bold()
        .underline(),
        quote.close,
        quote.open,
        quote.high,
        quote.low,
        fmt_num(&(quote.turnover / 100.0)),
        fmt_num(&quote.volume),
        quote.name,
    );
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[tokio::test]
    async fn test_check_symbol() {
        assert!(check_symbol("sz000001").await.is_ok());
        assert!(check_symbol("hk00700").await.is_ok());
        assert!(check_symbol("sh666666").await.is_err());
        assert!(check_symbol("hk70000").await.is_err());
        assert!(check_symbol("").await.is_err());
        assert!(check_symbol("bj").await.is_err());
    }
}
