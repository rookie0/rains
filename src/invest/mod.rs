use std::{str::FromStr, sync::LazyLock};

use anyhow::{Error, bail};
use regex::Regex;

pub mod quote;
pub mod stock;

static EXCHANGE_SYMBOL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^((SZ|SH|BJ)\d{6}|HK(\d{5}|[A-Z]{3}))$").unwrap());
static US_SYMBOL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[$.]?(?:[A-Z]{1,5}|[A-Z]{1,4}\.[A-Z])$").unwrap());

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
        let prefix = prefix.to_ascii_uppercase();
        match prefix.as_str() {
            "SH" => Ok(Exchange::Sse),
            "SZ" => Ok(Exchange::SZse),
            "BJ" => Ok(Exchange::Bse),
            "HK" => Ok(Exchange::HKex),
            _ if prefix.starts_with('$') => Ok(Exchange::Nasdaq),
            _ => bail!("不支持的交易所：{}", prefix),
        }
    }
}

impl FromStr for Investment {
    type Err = Error;

    fn from_str(symbol: &str) -> Result<Self, Self::Err> {
        let mut invest = Investment { market: Some(Market::Stock), ..Default::default() };
        // 证券代码协议仅允许 ASCII；避免 Unicode 大小写折叠把同形字符变成另一只证券。
        let normalized = symbol.to_ascii_uppercase();
        if let Some(caps) = EXCHANGE_SYMBOL_RE.captures(&normalized) {
            let symbol = caps.get(0).unwrap().as_str();
            invest.symbol = symbol.to_string();
            invest.code = symbol[2..].to_string();
            match Exchange::from_str(&symbol[..2]) {
                Ok(ex) => invest.exchange = Some(ex),
                Err(err) => bail!(err),
            }
            return Ok(invest);
        } else if let Some(caps) = US_SYMBOL_RE.captures(&normalized) {
            let symbol = caps.get(0).unwrap().as_str();
            invest.symbol = fmt_us_symbol(symbol);
            invest.code = invest.symbol[1..].to_string();
            invest.exchange = Some(Exchange::Nasdaq);
            return Ok(invest);
        }

        bail!("无法识别该股票代码")
    }
}

/// 美股代码格式 加前缀 $
pub fn fmt_us_symbol(symbol: &str) -> String {
    let code = symbol.strip_prefix(['$', '.']).unwrap_or(symbol);
    format!("${}", code.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Exchange 解析测试 ====================

    #[test]
    fn test_exchange_from_str() {
        assert_eq!(Exchange::from_str("SH").unwrap(), Exchange::Sse);
        assert_eq!(Exchange::from_str("sh").unwrap(), Exchange::Sse);
        assert_eq!(Exchange::from_str("SZ").unwrap(), Exchange::SZse);
        assert_eq!(Exchange::from_str("sz").unwrap(), Exchange::SZse);
        assert_eq!(Exchange::from_str("BJ").unwrap(), Exchange::Bse);
        assert_eq!(Exchange::from_str("bj").unwrap(), Exchange::Bse);
        assert_eq!(Exchange::from_str("HK").unwrap(), Exchange::HKex);
        assert_eq!(Exchange::from_str("hk").unwrap(), Exchange::HKex);
        assert_eq!(Exchange::from_str("$").unwrap(), Exchange::Nasdaq);
    }

    #[test]
    fn test_exchange_from_str_invalid() {
        assert!(Exchange::from_str("XX").is_err());
        assert!(Exchange::from_str("NYC").is_err());
        // 空字符串会返回错误
        assert!(Exchange::from_str("").is_err());
    }

    // ==================== Investment 解析测试 ====================

    #[test]
    fn test_investment_a_share() {
        // 上证
        let invest = Investment::from_str("sh601318").unwrap();
        assert_eq!(invest.symbol, "SH601318");
        assert_eq!(invest.code, "601318");
        assert_eq!(invest.exchange, Some(Exchange::Sse));
        assert_eq!(invest.market, Some(Market::Stock));

        // 深证
        let invest = Investment::from_str("sz000001").unwrap();
        assert_eq!(invest.symbol, "SZ000001");
        assert_eq!(invest.code, "000001");
        assert_eq!(invest.exchange, Some(Exchange::SZse));

        // 北证
        let invest = Investment::from_str("bj430047").unwrap();
        assert_eq!(invest.symbol, "BJ430047");
        assert_eq!(invest.code, "430047");
        assert_eq!(invest.exchange, Some(Exchange::Bse));
    }

    #[test]
    fn test_investment_hk_share() {
        // 港股 5位数字
        let invest = Investment::from_str("hk00700").unwrap();
        assert_eq!(invest.symbol, "HK00700");
        assert_eq!(invest.code, "00700");
        assert_eq!(invest.exchange, Some(Exchange::HKex));

        // 港股指数 3位字母
        let invest = Investment::from_str("hkHSI").unwrap();
        assert_eq!(invest.symbol, "HKHSI");
        assert_eq!(invest.code, "HSI");
        assert_eq!(invest.exchange, Some(Exchange::HKex));
    }

    #[test]
    fn test_investment_us_share() {
        // 不带 $ 前缀
        let invest = Investment::from_str("BABA").unwrap();
        assert_eq!(invest.symbol, "$BABA");
        assert_eq!(invest.code, "BABA");
        assert_eq!(invest.exchange, Some(Exchange::Nasdaq));

        // 带 $ 前缀
        let invest = Investment::from_str("$AAPL").unwrap();
        assert_eq!(invest.symbol, "$AAPL");
        assert_eq!(invest.code, "AAPL");
        assert_eq!(invest.exchange, Some(Exchange::Nasdaq));

        // 小写
        let invest = Investment::from_str("tsla").unwrap();
        assert_eq!(invest.symbol, "$TSLA");
        assert_eq!(invest.exchange, Some(Exchange::Nasdaq));

        // 带点的代码
        let invest = Investment::from_str("BRK.A").unwrap();
        assert_eq!(invest.symbol, "$BRK.A");
        assert_eq!(invest.exchange, Some(Exchange::Nasdaq));
    }

    #[test]
    fn test_investment_invalid() {
        // 空字符串
        assert!(Investment::from_str("").is_err());

        // 纯数字
        assert!(Investment::from_str("123456").is_err());

        // 特殊字符
        assert!(Investment::from_str("@#$%").is_err());

        // 不接受合法代码后的尾随内容
        assert!(Investment::from_str("SH601318garbage").is_err());
        assert!(Investment::from_str("AAPL1").is_err());
        assert!(Investment::from_str("A.").is_err());
        assert!(Investment::from_str("A..B").is_err());
        assert!(Investment::from_str("A....").is_err());
        assert!(Investment::from_str("ß").is_err());
    }

    #[test]
    fn test_investment_case_insensitive() {
        let invest1 = Investment::from_str("SH601318").unwrap();
        let invest2 = Investment::from_str("sh601318").unwrap();
        let invest3 = Investment::from_str("Sh601318").unwrap();

        assert_eq!(invest1.symbol, invest2.symbol);
        assert_eq!(invest2.symbol, invest3.symbol);
    }

    // ==================== fmt_us_symbol 测试 ====================

    #[test]
    fn test_fmt_us_symbol() {
        assert_eq!(fmt_us_symbol("BABA"), "$BABA");
        assert_eq!(fmt_us_symbol("$BABA"), "$BABA");
        assert_eq!(fmt_us_symbol("aapl"), "$AAPL");
        assert_eq!(fmt_us_symbol(".DJI"), "$DJI");
    }
}
