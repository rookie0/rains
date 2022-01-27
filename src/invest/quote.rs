/// 行情报价
#[derive(Debug, Default)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    /// 当前价
    pub now: String,
    /// 昨收
    pub close: String,
    /// 今开
    pub open: String,
    /// 最高
    pub high: String,
    /// 最低
    pub low: String,
    /// 竞买价
    pub buy: String,
    /// 竞卖价
    pub sell: String,
    /// 成交量
    pub turnover: String,
    /// 成交额
    pub volume: String,
    pub date: String,
    pub time: String,
    // pub currency: String,
}

impl From<&str> for Quote {
    fn from(str: &str) -> Self {
        let values: Vec<&str> = str.split(',').collect::<Vec<&str>>();
        Quote {
            symbol: "".to_string(),
            name: values.get(0).unwrap_or(&"").to_string(),
            now: values.get(3).unwrap_or(&"").to_string(),
            close: values.get(2).unwrap_or(&"").to_string(),
            open: values.get(1).unwrap_or(&"").to_string(),
            high: values.get(4).unwrap_or(&"").to_string(),
            low: values.get(5).unwrap_or(&"").to_string(),
            buy: values.get(6).unwrap_or(&"").to_string(),
            sell: values.get(7).unwrap_or(&"").to_string(),
            turnover: values.get(8).unwrap_or(&"").to_string(),
            volume: values.get(9).unwrap_or(&"").to_string(),
            date: values.get(30).unwrap_or(&"").to_string(),
            time: values.get(31).unwrap_or(&"").to_string(),
        }
    }
}
