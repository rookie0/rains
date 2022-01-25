#[derive(Debug, Default)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub now: String,
    pub close: String,
    pub open: String,
    pub high: String,
    pub low: String,
    pub buy: String,
    pub sell: String,
    pub turnover: String,
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
