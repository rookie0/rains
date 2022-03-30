use std::str::FromStr;

use anyhow::{bail, Error};
use regex::Regex;

pub mod quote;
pub mod stock;

#[derive(Debug, Default, Clone)]
pub struct Investment {
    pub code: String,
    pub symbol: String,
    pub name: String,
    pub market: Option<Market>,
    pub exchange: Option<Exchange>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Exchange {
    /// 上证
    Sse,
    /// 深证
    SZse,
    /// 北证
    Bse,
    /// 港交所
    HKex,
    /// 纽交所
    Nyse,
    /// 纳斯达克
    Nasdaq,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Market {
    /// 股票
    Stock,
    /// 基金
    Fund,
}

impl FromStr for Exchange {
    type Err = Error;

    fn from_str(prefix: &str) -> Result<Self, Self::Err> {
        let prefix = prefix.to_uppercase();
        match prefix.as_str() {
            "SH" => Ok(Exchange::Sse),
            "SZ" => Ok(Exchange::SZse),
            "BJ" => Ok(Exchange::Bse),
            "HK" => Ok(Exchange::HKex),
            _ => bail!("不支持的交易所：{}", prefix),
        }
    }
}

impl FromStr for Investment {
    type Err = Error;

    fn from_str(symbol: &str) -> Result<Self, Self::Err> {
        // ^((SZ|SH|BJ)\d{6}|HK\d{5}|[A-Z][A-Z.]{0,4})
        match Regex::new(r"^((SZ|SH|BJ)\d{6}|HK\d{5})").unwrap().captures(&symbol.to_uppercase()) {
            Some(caps) => {
                let symbol = caps.get(0).unwrap().as_str();
                let mut invest = Investment {
                    code: symbol[2..].to_string(),
                    symbol: symbol.to_string(),
                    market: Some(Market::Stock),
                    ..Default::default()
                };
                match Exchange::from_str(&symbol[..2]) {
                    Ok(ex) => invest.exchange = Some(ex),
                    Err(err) => bail!(err),
                }

                Ok(invest)
            }
            _ => bail!("当前仅支持沪深北证港股股票"),
        }
    }
}
