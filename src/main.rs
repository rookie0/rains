use anyhow::{bail, Result};
use owo_colors::OwoColorize;
use rains::{
    cli::{Opts, Subcommand},
    invest::quote::Quote,
    sina::Sina,
};
use regex::Regex;
use tracing::{debug, Level};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{}", err);
        std::process::exit(0x0100)
    }
}

async fn run() -> Result<()> {
    let args = Opts::parse_args()?;
    if args.debug {
        tracing_subscriber::fmt().with_max_level(Level::DEBUG).init();
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
            Err(err) => eprintln!("{}", err),
        },
        Subcommand::Info { symbol, .. } => match check_symbol(&symbol) {
            Ok(symbol) => {
                let code = &symbol[2..];
                match Sina::default().profile(code).await {
                    Ok(profile) => {
                        println!("证券代码：\t{}\n公司名称：\t{}\n主营业务：\t{}\n公司网址：\t{}\n办公地址：\t{}\n上市日期：\t{}\n发行价格：\t{}\n简称历史：\t{}",
                                 symbol,
                                 profile.name,
                                 profile.business,
                                 profile.website.underline(),
                                 profile.business_address,
                                 profile.listing_date,
                                 profile.listing_price,
                                 profile.used_name,
                        );
                    }
                    Err(err) => eprintln!("{}", err),
                }

                // todo other info
            }
            Err(err) => eprintln!("{}", err),
        },
        Subcommand::Quote { symbol, realtime } => match check_symbol(&symbol) {
            Ok(symbol) => {
                if realtime {
                    Sina::default()
                        .quote_ws(&symbol, |quote: Quote| {
                            write_quote(&quote);
                        })
                        .await;
                } else {
                    match Sina::default().quote(&symbol).await {
                        Ok(quote) => {
                            write_quote(&quote);
                        }
                        Err(err) => eprintln!("{}", err),
                    }
                }
            }
            Err(err) => eprintln!("{}", err),
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
        _ => {
            format!("{:.2}万", num / 10_000.0)
        }
    }
}

fn write_quote(quote: &Quote) {
    let now = quote.now.parse::<f64>().unwrap();
    let close = quote.close.parse::<f64>().unwrap();
    let rate = (now / close - 1.0) * 100.0;
    let now = format!("￥{:.2} {:.2}%", now, rate);
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
        close,
        quote.open.parse::<f64>().unwrap(),
        quote.high.parse::<f64>().unwrap(),
        quote.low.parse::<f64>().unwrap(),
        fmt_num(&(quote.turnover.parse::<f64>().unwrap() / 100.0)),
        fmt_num(&quote.volume.parse::<f64>().unwrap()),
        quote.name,
    );
}
