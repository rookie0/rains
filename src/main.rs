use std::io::{stdout, Write};

use anyhow::{bail, Result};
use crossterm::{cursor, style::Stylize, ExecutableCommand};
use once_cell::sync::Lazy;
use owo_colors::OwoColorize;
use rains::{
    cli::{Opts, Subcommand},
    invest::quote::Quote,
    sina::Sina,
};
use regex::Regex;
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
                    println!("{} \t {}", invest.symbol, invest.name);
                }
            }
            Err(err) => error!("{}", err),
        },
        Subcommand::Info { symbol, all, financials, structure, dividends, presses } => match check_symbol(&symbol) {
            Ok(symbol) => {
                println!("{}", "基本信息".bold());
                match SINA.lock().await.profile(&symbol).await {
                    Ok(profile) => println!(
                        "证券代码\t{}\n简称历史\t{}\n公司名称\t{}\n上市日期\t{}\n发行价格\t{:.2}\n行业分类\t{}\n主营业务\t{}\n办公地址\t{}\n公司网址\t{}\n当前价格\t{:.2}\n市净率PB\t{:.2}\n市盈率TTM\t{}\n总市值  \t{}\n流通市值\t{}",
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
                                let cols = vec![
                                    "截止日期     ",
                                    "总营收       ",
                                    "净利润       ",
                                    "每股净资产    ",
                                    "每股资本公积金",
                                ];
                                for (i, col) in cols.iter().enumerate() {
                                    let mut output = String::from(*col);
                                    for f in financials.iter() {
                                        match i {
                                            0 => output.push_str(&format!(" \t\t {}", f.date)),
                                            1 => output.push_str(&format!(" \t\t {}", fmt_num(&f.total_revenue))),
                                            2 => output.push_str(&format!(" \t\t {}", fmt_num(&f.net_profit))),
                                            3 => output.push_str(&format!(" \t\t {:.4}", f.ps_net_assets)),
                                            4 => output.push_str(&format!(" \t\t {:.4}", f.ps_capital_reserve)),
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
                }

                if all || dividends {
                    println!("\n{}", "分红送配".bold());
                    let symbol = symbol.clone();
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
                }
            }
            Err(err) => error!("{}", err),
        },
        Subcommand::Quote { symbol, realtime, multiline } => match check_symbol(&symbol) {
            Ok(symbol) => {
                if realtime {
                    Sina::quote_ws(&symbol, |quote: Quote| {
                        if !multiline {
                            let mut stdout = stdout();
                            write_quote(&quote);
                            stdout.execute(cursor::MoveToPreviousLine(1)).unwrap();
                            stdout.flush().unwrap();
                        } else {
                            write_quote(&quote);
                        }
                    })
                    .await;
                } else {
                    match Sina::default().quote(&symbol).await {
                        Ok(quote) => {
                            write_quote(&quote);
                        }
                        Err(err) => error!("{}", err),
                    }
                }
            }
            Err(err) => error!("{}", err),
        },
    }

    Ok(())
}

fn check_symbol(symbol: &str) -> Result<String> {
    match Regex::new(r"^(SZ|SH|BJ)\d{6}").unwrap().captures(&symbol.to_uppercase()) {
        Some(caps) if symbol.len() == 8 => Ok(caps.get(0).unwrap().as_str().to_uppercase()),
        _ => bail!("当前仅支持沪深及北证股票信息查询"),
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
        "{} {}  {}\t昨收：{:.2}\t今开：{:.2}\t最高：{:.2}\t最低：{:.2}\t成交量：{}手\t成交额：{}元\t{}",
        quote.date,
        quote.time,
        match rate {
            _ if rate > 0.0 => {
                now.red().to_string()
            }
            _ if rate < 0.0 => {
                now.green().to_string()
            }
            _ => {
                now.default_color().to_string()
            }
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
