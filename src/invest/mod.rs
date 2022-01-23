use std::str::FromStr;

use anyhow::{bail, Error};

pub mod quote;
pub mod stock;

#[derive(Debug, Default)]
pub struct Investment {
    pub code: String,
    pub symbol: String,
    pub name: String,
    pub market: Option<Market>,
    pub exchange: Option<Exchange>,
}

#[derive(Debug)]
pub enum Exchange {
    /// 上证
    Sse,
    /// 深证
    SZse,
    /// 北证
    Bse,
    /// 港交所
    HKex,
}

#[derive(Debug)]
pub enum Market {
    /// 股票
    Stock,
    /// 基金
    Fund,
}

impl FromStr for Exchange {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s[..2].to_uppercase().as_str() {
            "SH" => Ok(Exchange::Sse),
            "SZ" => Ok(Exchange::SZse),
            "BJ" => Ok(Exchange::Bse),
            "HK" => Ok(Exchange::HKex),
            _ => bail!("unsupported or invalid"),
        }
    }
}
