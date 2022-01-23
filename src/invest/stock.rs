#[derive(Debug, Default)]
pub struct Stock {
    pub symbol: String,
    pub profile: Profile,
    pub financials: Vec<Financial>,
    pub structure: Vec<Structure>,
    pub dividends: Vec<Dividend>,
    pub presses: Vec<Press>,
}

#[derive(Debug, Default)]
pub struct Profile {
    pub name: String,
    pub used_name: String,
    pub listing_price: String,
    pub listing_date: String,
    // pub category: String,
    pub business: String,
    pub business_address: String,
    pub website: String,
}

#[derive(Debug, Default)]
pub struct Financial {}

#[derive(Debug, Default)]
pub struct Structure {}

#[derive(Debug, Default)]
pub struct Dividend {}

#[derive(Debug, Default)]
pub struct Press {}
