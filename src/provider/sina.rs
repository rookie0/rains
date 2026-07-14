//! 新浪财经数据源实现

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::LazyLock,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use encoding_rs::{Encoding, GB18030, UTF_8};
use futures_util::{SinkExt, StreamExt};
use regex::Regex;
use reqwest::{
    Client, StatusCode, Url,
    header::{self as reqwest_header, HeaderMap, HeaderValue},
};
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use serde_json::Value;
use tokio::{
    join, select,
    time::{Instant, interval_at, timeout},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue as WsHeaderValue, header as ws_header},
        protocol::WebSocketConfig,
    },
};
use tracing::debug;

use super::{MAX_QUOTE_SYMBOLS, Provider, RealtimeProvider};
use crate::invest::{
    Exchange, Investment, Market, fmt_us_symbol,
    quote::Quote,
    stock::{Dividend, Financial, Holder, Press, Profile, Structure},
};

const PORTAL: &str = "https://finance.sina.com.cn";
const TIMEOUT_SECS: u64 = 10;
const INITIAL_QUOTES_TIMEOUT_SECS: u64 = 10;
const WS_HEARTBEAT_SECS: u64 = 60;
const MAX_HTTP_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_WS_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_SEARCH_QUERY_BYTES: usize = 256;
const FINANCIAL_REPORT_COUNT: usize = 8;

static QUOTED_VALUE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"\r\n]*)""#).unwrap());
static SNAPSHOT_QUOTE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"hq_str_(?:rt_)?(?:gb_)?([A-Za-z0-9$]+)="([^"\r\n]*)""#).unwrap());
static REALTIME_QUOTE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:rt_)?(?:gb_)?([A-Za-z0-9$]+)=([^\r\n]*)(?:\r?\n|$)").unwrap());
static NUMBER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[-+]?(?:(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?|\.\d+)").unwrap());
static PRESS_BASE_URL: LazyLock<Url> = LazyLock::new(|| Url::parse("https://vip.stock.finance.sina.com.cn/").unwrap());

#[derive(Debug)]
struct QuoteRequest {
    formatted: String,
    expected_order: Vec<String>,
    expected_set: HashSet<String>,
}

impl QuoteRequest {
    fn len(&self) -> usize {
        self.expected_order.len()
    }
}

#[derive(Debug, Deserialize)]
struct FinanceApiResponse {
    result: FinanceApiResult,
}

#[derive(Debug, Deserialize)]
struct FinanceApiResult {
    status: FinanceApiStatus,
    data: Option<FinanceApiData>,
}

#[derive(Debug, Deserialize)]
struct FinanceApiStatus {
    code: i64,
    #[serde(default)]
    msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FinanceApiData {
    report_list: HashMap<String, FinanceApiReport>,
}

#[derive(Debug, Deserialize)]
struct FinanceApiReport {
    data: Vec<FinanceApiItem>,
}

#[derive(Debug, Deserialize)]
struct FinanceApiItem {
    item_field: String,
    item_title: String,
    item_value: Option<String>,
    #[serde(default)]
    item_tongbi: Option<Value>,
}

fn validate_search_query(query: &str) -> Result<&str> {
    let query = query.trim();
    if query.is_empty() {
        bail!("搜索关键字不能为空");
    }
    if query.len() > MAX_SEARCH_QUERY_BYTES {
        bail!("搜索关键字超过 {} 字节限制", MAX_SEARCH_QUERY_BYTES);
    }
    if query.chars().any(char::is_control) {
        bail!("搜索关键字包含控制字符");
    }
    Ok(query)
}

fn validate_stock_code(code: &str) -> Result<&str> {
    let code = code.trim();
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("无效的 A 股代码: {code}");
    }
    Ok(code)
}

fn validate_profile_symbol(symbol: &str) -> Result<Investment> {
    let invest = Investment::from_str(symbol.trim()).context("无效的公司证券代码")?;
    if !matches!(invest.exchange, Some(Exchange::Sse | Exchange::SZse | Exchange::Bse)) {
        bail!("公司资料仅支持沪深北证 A 股");
    }
    validate_stock_code(&invest.code)?;
    Ok(invest)
}

/// 新浪财经数据提供者
#[derive(Debug)]
pub struct SinaProvider {
    client: Client,
}

impl Default for SinaProvider {
    fn default() -> Self {
        ensure_rustls_provider();
        let mut headers = HeaderMap::new();
        headers.insert(reqwest_header::REFERER, HeaderValue::from_static(PORTAL));

        let client =
            Client::builder().default_headers(headers).timeout(Duration::from_secs(TIMEOUT_SECS)).build().unwrap();

        Self { client }
    }
}

impl SinaProvider {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// HTTP 请求 (根据 Content-Type 或 URL 自动处理编码)
    async fn request(&self, url: &str) -> Result<String> {
        let mut resp = self.client.get(url).send().await?;
        let status = resp.status();

        if status != StatusCode::OK {
            bail!("request return error, http code: {}", status);
        }

        // 检查 Content-Type 中的 charset
        let content_type = resp
            .headers()
            .get(reqwest_header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        if resp.content_length().is_some_and(|length| length > MAX_HTTP_RESPONSE_BYTES as u64) {
            bail!("响应正文超过 {} 字节限制", MAX_HTTP_RESPONSE_BYTES);
        }
        let mut bytes =
            Vec::with_capacity(resp.content_length().unwrap_or(0).min(MAX_HTTP_RESPONSE_BYTES as u64) as usize);
        while let Some(chunk) = resp.chunk().await? {
            let new_len = bytes
                .len()
                .checked_add(chunk.len())
                .filter(|length| *length <= MAX_HTTP_RESPONSE_BYTES)
                .context("响应正文超过大小限制")?;
            bytes.reserve(new_len - bytes.len());
            bytes.extend_from_slice(&chunk);
        }
        decode_response(&bytes, &content_type, url)
    }
}

fn ensure_rustls_provider() {
    // 显式选择 provider，避免下游同时启用 ring/aws-lc 时由 rustls 隐式推断并 panic。
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[async_trait]
impl Provider for SinaProvider {
    /// 搜索投资品
    ///
    /// 类型码说明:
    /// - 11 A股, 12 B股, 13 权证, 14 期货, 15 债券
    /// - 21 开基, 22 ETF, 23 LOF, 24 货基, 25 QDII, 26 封基
    /// - 31 港股, 32 窝轮, 33 港指数
    /// - 41 美股, 42 外期
    /// - 81/82 债券
    async fn search(&self, query: &str) -> Result<Vec<Investment>> {
        let query = validate_search_query(query)?;
        // 该接口把参数放在 path 而非标准 query 中，只对用户输入部分做百分号编码。
        let mut encoder = Url::parse("https://localhost/")?;
        encoder.query_pairs_mut().append_pair("key", query);
        let encoded_query =
            encoder.query().and_then(|query| query.strip_prefix("key=")).context("搜索关键字编码失败")?;
        let url =
            format!("https://suggest3.sinajs.cn/suggest/type=11,12,15,21,22,23,24,25,26,31,33,41&key={encoded_query}");

        let content = self.request(&url).await?;
        debug!(response_bytes = content.len(), "search response received");
        Ok(parse_search_results(&content))
    }

    async fn profile(&self, symbol: &str) -> Result<Profile> {
        let invest = validate_profile_symbol(symbol)?;
        let symbol = invest.symbol;
        let code = invest.code;
        let corp_url = format!("https://vip.stock.finance.sina.com.cn/corp/go.php/vCI_CorpInfo/stockid/{}.phtml", code);
        let info_url = format!("https://hq.sinajs.cn/list={},{}_i", symbol.to_lowercase(), symbol.to_lowercase());

        let (corp, info) = join!(self.request(&corp_url), self.request(&info_url));
        let corp = corp.with_context(|| format!("获取 {symbol} 公司资料失败"))?;
        let info = info.with_context(|| format!("获取 {symbol} 行情扩展资料失败"))?;

        let profile = parse_stock_info(&info, parse_corp_info(&corp, Profile::default()));
        if profile.name.trim().is_empty() {
            bail!("{symbol} 公司资料响应缺少公司名称");
        }
        Ok(profile)
    }

    async fn financials(&self, code: &str) -> Result<Vec<Financial>> {
        let code = validate_stock_code(code)?;
        let url = format!(
            "https://quotes.sina.cn/cn/api/openapi.php/CompanyFinanceService.getFinanceReport2022?paperCode={code}&source=gjzb&type=0&page=1&num={FINANCIAL_REPORT_COUNT}"
        );

        let content = self.request(&url).await?;
        parse_financials(&content).with_context(|| format!("解析 {code} 财务数据失败"))
    }

    async fn structures(&self, code: &str) -> Result<Vec<Structure>> {
        let code = validate_stock_code(code)?;
        let url = format!("https://vip.stock.finance.sina.com.cn/corp/go.php/vCI_StockHolder/stockid/{}.phtml", code);

        let content = self.request(&url).await?;
        Ok(parse_structures(&content))
    }

    async fn dividends(&self, code: &str) -> Result<Vec<Dividend>> {
        let code = validate_stock_code(code)?;
        let url = format!("https://vip.stock.finance.sina.com.cn/corp/go.php/vISSUE_ShareBonus/stockid/{}.phtml", code);

        let content = self.request(&url).await?;
        Ok(parse_dividends(&content))
    }

    async fn presses(&self, code: &str) -> Result<Vec<Press>> {
        let code = validate_stock_code(code)?;
        let url = format!("https://vip.stock.finance.sina.com.cn/corp/go.php/vCB_AllBulletin/stockid/{}.phtml", code);

        let content = self.request(&url).await?;
        Ok(parse_presses(&content))
    }

    async fn quotes(&self, symbols: &str) -> Result<Vec<Quote>> {
        let request = prepare_quote_request(symbols)?;
        let url = format!("https://hq.sinajs.cn/list={}", request.formatted);

        let content = self.request(&url).await?;
        debug!(response_bytes = content.len(), "quotes response received");

        ensure_complete_quotes(parse_quotes(&SNAPSHOT_QUOTE_RE, &content), &request)
    }
}

#[async_trait]
impl RealtimeProvider for SinaProvider {
    async fn quotes_realtime<F>(&self, symbols: &str, mut handler: F) -> Result<()>
    where
        F: FnMut(Vec<Quote>) -> Result<()> + Send + 'static,
    {
        ensure_rustls_provider();
        let request = prepare_quote_request(symbols)?;
        let url = format!("wss://hq.sinajs.cn/wskt?list={}", request.formatted);

        let mut req = url.into_client_request().context("无法构造实时行情 WebSocket 请求")?;
        req.headers_mut().insert(ws_header::ORIGIN, WsHeaderValue::from_static(PORTAL));

        let mut ws_config = WebSocketConfig::default();
        ws_config.max_message_size = Some(MAX_WS_MESSAGE_BYTES);
        ws_config.max_frame_size = Some(MAX_WS_MESSAGE_BYTES);
        let (ws, _) =
            timeout(Duration::from_secs(TIMEOUT_SECS), connect_async_with_config(req, Some(ws_config), false))
                .await
                .context("连接实时行情服务超时")?
                .context("连接实时行情服务失败")?;
        let (mut sender, mut receiver) = ws.split();
        let heartbeat_period = Duration::from_secs(WS_HEARTBEAT_SECS);
        let mut heartbeat = interval_at(Instant::now() + heartbeat_period, heartbeat_period);
        let initial_timeout =
            tokio::time::sleep_until(Instant::now() + Duration::from_secs(INITIAL_QUOTES_TIMEOUT_SECS));
        tokio::pin!(initial_timeout);
        let mut initial_received = HashSet::new();

        loop {
            select! {
                msg = receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            let quotes = filter_requested_quotes(
                                parse_quotes(&REALTIME_QUOTE_RE, text.as_ref()),
                                &request,
                            );
                            if !quotes.is_empty() {
                                initial_received.extend(quotes.iter().map(|quote| quote.symbol.clone()));
                                handler(quotes)?;
                            }
                        }
                        Some(Ok(Message::Close(frame))) => {
                            let _ = timeout(Duration::from_secs(TIMEOUT_SECS), sender.flush()).await;
                            bail!("实时行情连接被服务端关闭: {frame:?}");
                        }
                        Some(Ok(_)) => {}
                        Some(Err(err)) => return Err(err).context("读取实时行情失败"),
                        None => bail!("实时行情连接意外结束"),
                    }
                }
                _ = &mut initial_timeout, if initial_received.len() < request.len() => {
                    let missing = missing_symbols(&request, &initial_received);
                    bail!(
                        "实时行情首轮数据等待超时，缺少: {}",
                        missing.join(",")
                    );
                }
                _ = heartbeat.tick() => {
                    timeout(
                        Duration::from_secs(TIMEOUT_SECS),
                        sender.send(Message::Text(String::new().into())),
                    )
                        .await
                        .context("发送实时行情心跳超时")?
                        .context("发送实时行情心跳失败")?;
                }
            }
        }
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

fn decode_response(bytes: &[u8], content_type: &str, url: &str) -> Result<String> {
    let charset = content_type.split(';').skip(1).find_map(|parameter| {
        let (name, value) = parameter.trim().split_once('=')?;
        name.trim().eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches(['\'', '"']))
    });

    let encoding: &'static Encoding = if let Some(charset) = charset {
        Encoding::for_label(charset.as_bytes()).with_context(|| format!("不支持的响应字符集: {charset}"))?
    } else if url.contains("sinajs.cn") {
        GB18030
    } else {
        UTF_8
    };

    let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
    if had_errors {
        bail!("响应正文不是有效的 {} 编码", encoding.name());
    }
    Ok(text.into_owned())
}

fn parse_search_results(content: &str) -> Vec<Investment> {
    let Some(caps) = QUOTED_VALUE_RE.captures(content) else {
        return Vec::new();
    };
    let Some(matched) = caps.get(1).map(|value| value.as_str()).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };

    let mut investments = Vec::new();

    // 格式: 名称,类型码,代码,代码,名称,,名称,?,在市,?
    for piece in matched.split(';') {
        let fields: Vec<&str> = piece.split(',').collect();
        if fields.len() < 9 || fields[8] != "1" {
            continue;
        }

        let type_code = fields[1];
        let mut symbol = fields[3].to_uppercase();
        let (market, exchange) = match type_code {
            "11" | "12" => {
                let exchange = symbol.get(..2).and_then(|prefix| Exchange::from_str(prefix).ok());
                (Some(Market::Stock), exchange)
            }
            // 当前领域模型没有债券类型；不能把 type=15 债券误标为股票。
            "15" => continue,
            "21" | "22" | "23" | "24" | "25" | "26" => (Some(Market::Fund), None),
            "31" | "33" => {
                symbol = format!("HK{symbol}");
                (Some(Market::Stock), Some(Exchange::HKex))
            }
            "41" => {
                symbol = fmt_us_symbol(&symbol);
                (Some(Market::Stock), Some(Exchange::Nasdaq))
            }
            _ => continue,
        };

        investments.push(Investment {
            code: fields[2].to_string(),
            symbol,
            name: fields[4].to_string(),
            market,
            exchange,
        });
    }

    investments
}

/// 格式化行情代码
/// - 港股格式: rt_hk00700
/// - 港股指数: rt_hkHSI
/// - 美股格式: gb_baba
fn prepare_quote_request(symbols: &str) -> Result<QuoteRequest> {
    let mut formatted = Vec::new();
    let mut expected_order = Vec::new();
    let mut expected_set = HashSet::new();

    for symbol in symbols.split(',').map(str::trim).filter(|symbol| !symbol.is_empty()) {
        let invest = Investment::from_str(symbol).with_context(|| format!("无效的行情代码: {symbol}"))?;
        if expected_set.contains(&invest.symbol) {
            continue;
        }
        if expected_order.len() >= MAX_QUOTE_SYMBOLS {
            bail!("单次最多查询 {MAX_QUOTE_SYMBOLS} 个证券代码");
        }

        formatted.push(match invest.exchange {
            Some(Exchange::HKex) => format!("rt_hk{}", invest.code),
            Some(Exchange::Nasdaq) => format!("gb_{}", invest.code.replace('.', "$").to_lowercase()),
            _ => invest.symbol.to_lowercase(),
        });
        expected_set.insert(invest.symbol.clone());
        expected_order.push(invest.symbol);
    }

    if formatted.is_empty() {
        bail!("没有有效的行情代码");
    }
    Ok(QuoteRequest { formatted: formatted.join(","), expected_order, expected_set })
}

#[cfg(test)]
fn format_quote_symbols(symbols: &str) -> Result<String> {
    prepare_quote_request(symbols).map(|request| request.formatted)
}

fn filter_requested_quotes(quotes: Vec<Quote>, request: &QuoteRequest) -> Vec<Quote> {
    let mut by_symbol = HashMap::with_capacity(request.len());
    for quote in quotes {
        if request.expected_set.contains(&quote.symbol) {
            by_symbol.entry(quote.symbol.clone()).or_insert(quote);
        }
    }

    request.expected_order.iter().filter_map(|symbol| by_symbol.remove(symbol)).collect()
}

fn missing_symbols(request: &QuoteRequest, received: &HashSet<String>) -> Vec<String> {
    request.expected_order.iter().filter(|symbol| !received.contains(*symbol)).cloned().collect()
}

fn ensure_complete_quotes(quotes: Vec<Quote>, request: &QuoteRequest) -> Result<Vec<Quote>> {
    let quotes = filter_requested_quotes(quotes, request);
    let received: HashSet<_> = quotes.iter().map(|quote| quote.symbol.clone()).collect();
    let missing = missing_symbols(request, &received);
    if !missing.is_empty() {
        bail!("行情服务未返回有效数据: {}", missing.join(","));
    }
    Ok(quotes)
}

fn element_text(element: ElementRef<'_>) -> String {
    element.text().map(str::trim).filter(|text| !text.is_empty()).collect::<Vec<_>>().join(" ")
}

fn resolve_press_url(href: Option<&str>) -> String {
    let Some(href) = href.map(str::trim).filter(|href| !href.is_empty()) else {
        return String::new();
    };
    let Ok(url) = PRESS_BASE_URL.join(href) else {
        return String::new();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return String::new();
    }
    url.into()
}

/// 解析公司基本信息
fn parse_corp_info(content: &str, mut profile: Profile) -> Profile {
    let doc = Html::parse_document(content);
    let tds = Selector::parse("#comInfo1 td").unwrap();
    let a = Selector::parse("a").unwrap();

    for (i, td) in doc.select(&tds).enumerate() {
        match i {
            1 => profile.name = element_text(td),
            7 => {
                if let Some(link) = td.select(&a).next() {
                    profile.listing_date = element_text(link);
                }
            }
            9 => profile.listing_price = parse_number(&element_text(td)),
            35 => {
                if let Some(link) = td.select(&a).next() {
                    profile.website = element_text(link);
                }
            }
            41 => profile.used_name = element_text(td),
            45 => profile.business_address = element_text(td),
            49 => profile.business = element_text(td),
            _ => {}
        }
    }

    profile
}

/// 解析股票行情信息
fn parse_stock_info(content: &str, mut profile: Profile) -> Profile {
    for (i, caps) in QUOTED_VALUE_RE.captures_iter(content).enumerate() {
        let data = caps.get(1).unwrap().as_str();

        match i {
            0 => {
                // 基础行情
                if let Some(quote) = parse_quote_cn(data) {
                    profile.price = quote.now;
                    if profile.used_name.is_empty() {
                        profile.used_name = quote.name;
                    }
                }
            }
            1 => {
                // 扩展信息
                let fields: Vec<&str> = data.split(',').collect();
                let vps = fields.get(5).map_or(0.0, |value| parse_nonnegative_number(value));
                let cap = fields.get(7).map_or(0.0, |value| parse_nonnegative_number(value));
                let traded_cap = fields.get(8).map_or(0.0, |value| parse_nonnegative_number(value));
                // index 13 是最近四季度净利润之和；index 18 仅为最近报告期净利润。
                let ttm_profit = fields.get(13).map_or(0.0, |value| parse_nonnegative_number(value));

                profile.pb = if vps > 0.0 { profile.price / vps } else { 0.0 };
                profile.category = fields.get(34).map_or("", |value| value.trim()).to_string();
                profile.market_cap = profile.price * cap * 10000.0;
                profile.traded_market_cap = profile.price * traded_cap * 10000.0;

                if ttm_profit > 0.0 {
                    profile.pe_ttm = profile.market_cap / ttm_profit / 100_000_000.0;
                }
            }
            _ => {}
        }
    }

    profile
}

/// 解析新浪财务 JSON。核心展示字段必须存在且是有限数值，避免把 schema 漂移静默降级成 0。
fn parse_financials(content: &str) -> Result<Vec<Financial>> {
    let response: FinanceApiResponse = serde_json::from_str(content).context("财务接口返回的 JSON schema 无效")?;
    if response.result.status.code != 0 {
        let message = response.result.status.msg.as_deref().unwrap_or("未提供错误信息").escape_debug();
        bail!("财务接口返回错误状态 {}: {message}", response.result.status.code);
    }

    let data = response.result.data.context("财务接口未提供数据")?;
    if data.report_list.is_empty() {
        bail!("财务接口返回空报表");
    }

    let mut reports: Vec<_> = data.report_list.into_iter().collect();
    reports.sort_unstable_by(|(left, _), (right, _)| right.cmp(left));

    reports
        .into_iter()
        .take(4)
        .map(|(date, report)| financial_from_report(&date, &report).with_context(|| format!("财报 {date} 数据无效")))
        .collect()
}

fn financial_from_report(date: &str, report: &FinanceApiReport) -> Result<Financial> {
    let (total_revenue, total_revenue_rate) =
        required_metric(&report.data, is_revenue_title, &["BIZINCO", "BIZTOTINCO"], "营业收入")?;
    let (net_profit, net_profit_rate) = required_metric(
        &report.data,
        is_parent_net_profit_title,
        &["NETPARESHARPROF", "NETPARECOMPPROF", "PARENETP"],
        "归母净利润",
    )?;
    let (ps_net_assets, _) = required_metric(&report.data, is_net_assets_per_share_title, &["NAPS"], "每股净资产")?;
    let (ps_capital_reserve, _) =
        required_metric(&report.data, is_capital_reserve_per_share_title, &["CRPS"], "每股资本公积金")?;

    Ok(Financial {
        date: format_report_date(date)?,
        total_revenue,
        total_revenue_rate,
        net_profit,
        net_profit_rate,
        net_profit_after_nrgal: optional_metric(&report.data, &["NPCUT"], "扣非净利润")?,
        net_profit_after_nrgal_rate: optional_metric_growth(&report.data, &["NPCUT"], "扣非净利润")?,
        eps: optional_metric(&report.data, &["EPSBASIC"], "基本每股收益")?,
        eps_undistributed: optional_metric(&report.data, &["UPPS"], "每股未分配利润")?,
        ps_net_assets,
        ps_capital_reserve,
        ps_cash_flow: optional_metric(&report.data, &["OPNCFPS"], "每股经营现金流")?,
        roe: optional_metric(&report.data, &["ROEWEIGHTED"], "净资产收益率")?,
    })
}

fn required_metric(
    items: &[FinanceApiItem],
    title_matches: fn(&str) -> bool,
    fallback_fields: &[&str],
    label: &str,
) -> Result<(f64, f64)> {
    let item = find_metric(items, title_matches, fallback_fields).with_context(|| format!("缺少{label}字段"))?;
    let value = parse_required_item_value(item, label)?;
    let growth = parse_growth(item.item_tongbi.as_ref(), label)?;
    Ok((value, growth))
}

fn find_metric<'a>(
    items: &'a [FinanceApiItem],
    title_matches: fn(&str) -> bool,
    fallback_fields: &[&str],
) -> Option<&'a FinanceApiItem> {
    items
        .iter()
        .find(|item| title_matches(item.item_title.trim()))
        .or_else(|| items.iter().find(|item| fallback_fields.contains(&item.item_field.as_str())))
}

fn optional_metric(items: &[FinanceApiItem], fields: &[&str], label: &str) -> Result<f64> {
    let Some(item) = items.iter().find(|item| fields.contains(&item.item_field.as_str())) else {
        return Ok(0.0);
    };
    match item.item_value.as_deref() {
        Some(value) if !value.trim().is_empty() => parse_finite_number(value, label),
        _ => Ok(0.0),
    }
}

fn optional_metric_growth(items: &[FinanceApiItem], fields: &[&str], label: &str) -> Result<f64> {
    let Some(item) = items.iter().find(|item| fields.contains(&item.item_field.as_str())) else {
        return Ok(0.0);
    };
    parse_growth(item.item_tongbi.as_ref(), label)
}

fn parse_required_item_value(item: &FinanceApiItem, label: &str) -> Result<f64> {
    let value = item.item_value.as_deref().with_context(|| format!("{label}值为空"))?;
    if value.trim().is_empty() {
        bail!("{label}值为空");
    }
    parse_finite_number(value, label)
}

fn parse_finite_number(value: &str, label: &str) -> Result<f64> {
    let number: f64 = value.trim().parse().with_context(|| format!("{label}不是有效数值"))?;
    if !number.is_finite() {
        bail!("{label}不是有限数值");
    }
    Ok(number)
}

fn parse_growth(value: Option<&Value>, label: &str) -> Result<f64> {
    let ratio = match value {
        None | Some(Value::Null) => return Ok(0.0),
        Some(Value::Number(number)) => number.as_f64().with_context(|| format!("{label}同比不是有效数值"))?,
        Some(Value::String(value)) if value.trim().is_empty() => return Ok(0.0),
        Some(Value::String(value)) => parse_finite_number(value, &format!("{label}同比"))?,
        Some(_) => bail!("{label}同比字段类型无效"),
    };
    let percent = ratio * 100.0;
    if !percent.is_finite() {
        bail!("{label}同比不是有限数值");
    }
    Ok(percent)
}

fn format_report_date(date: &str) -> Result<String> {
    if date.len() != 8 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("财报日期格式无效: {date}");
    }
    let year: u16 = date[0..4].parse()?;
    let month: u8 = date[4..6].parse()?;
    let day: u8 = date[6..8].parse()?;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0 || day == 0 || day > days_in_month {
        bail!("财报日期格式无效: {date}");
    }
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn is_revenue_title(title: &str) -> bool {
    matches!(title, "营业收入" | "营业总收入")
}

fn is_parent_net_profit_title(title: &str) -> bool {
    matches!(title, "归母净利润" | "归属母公司净利润" | "归属母公司股东的净利润")
}

fn is_net_assets_per_share_title(title: &str) -> bool {
    title == "每股净资产"
}

fn is_capital_reserve_per_share_title(title: &str) -> bool {
    title == "每股资本公积金"
}

/// 解析股东结构
fn parse_structures(content: &str) -> Vec<Structure> {
    let doc = Html::parse_document(content);
    let trs = Selector::parse("#Table1 tbody tr").unwrap();
    let td1 = Selector::parse("td:last-child").unwrap();
    let td2 = Selector::parse("td div").unwrap();

    let mut structures = Vec::new();
    let mut s = Structure::default();

    // 最多保留最近四期；按完整的 17 行记录截断，避免在下一期中途 break。
    for (i, tr) in doc.select(&trs).take(17 * 4).enumerate() {
        let val = tr.select(&td1).next().map(element_text).unwrap_or_default();

        match i % 17 {
            0 => {
                if i > 0 {
                    structures.push(s);
                    s = Structure::default();
                }
                s.date = val;
            }
            3 => s.holders_num = parse_number(&val),
            4 => s.shares_avg = parse_number(&val),
            6..=15 => {
                let mut h = Holder::default();
                for (m, td) in tr.select(&td2).enumerate() {
                    let value = element_text(td);
                    match m {
                        1 => h.name = value,
                        2 => h.shares = parse_number(&value),
                        3 => h.percent = parse_number(&value),
                        4 => h.shares_type = value,
                        _ => {}
                    }
                }
                s.holders_ten.push(h);
            }
            _ => {}
        }
    }

    if !s.date.trim().is_empty() {
        structures.push(s);
    }

    structures
}

/// 解析分红数据
fn parse_dividends(content: &str) -> Vec<Dividend> {
    let doc = Html::parse_document(content);
    let tds = Selector::parse("#sharebonus_1 tr td").unwrap();

    let mut dividends = Vec::new();
    let mut d = Dividend::default();

    for (i, td) in doc.select(&tds).enumerate() {
        let val = element_text(td);

        match i % 9 {
            0 => {
                if i > 0 {
                    dividends.push(d);
                    d = Dividend::default();
                }
                d.date = val;
            }
            1 => d.shares_dividend = parse_number(&val),
            2 => d.shares_into = parse_number(&val),
            3 => d.money = parse_number(&val),
            5 => d.date_dividend = val,
            6 => d.date_record = val,
            _ => {}
        }
    }

    if !d.date.trim().is_empty() {
        dividends.push(d);
    }

    dividends
}

/// 解析公告列表
fn parse_presses(content: &str) -> Vec<Press> {
    let doc = Html::parse_document(content);
    let ul = Selector::parse("div.datelist ul").unwrap();
    let a = Selector::parse("a").unwrap();

    let mut presses = Vec::new();
    let mut date = String::new();

    if let Some(ele) = doc.select(&ul).next() {
        for item in ele.children() {
            if let Some(text) = item.value().as_text() {
                let text = text.trim();
                if !text.is_empty() {
                    date = text.to_string();
                }
                continue;
            }

            let Some(element) = ElementRef::wrap(item) else {
                continue;
            };
            let href = element
                .value()
                .attr("href")
                .or_else(|| element.select(&a).next().and_then(|link| link.value().attr("href")));
            let url = resolve_press_url(href);
            let title = element_text(element);
            if !date.is_empty() && !title.is_empty() && !url.is_empty() {
                presses.push(Press { date: date.clone(), title, url, ..Default::default() });
            }
        }
    }

    presses
}

/// 解析多个行情
fn parse_quotes(regex: &Regex, content: &str) -> Vec<Quote> {
    let mut quotes = Vec::new();

    for caps in regex.captures_iter(content) {
        let (Some(symbol), Some(payload)) = (caps.get(1), caps.get(2)) else {
            continue;
        };
        let symbol_str = symbol.as_str();
        let quote_str = payload.as_str();

        let normalized_symbol = if let Some((base, class)) = symbol_str.split_once('$') {
            format!("{base}.{class}")
        } else {
            symbol_str.to_string()
        };
        let Ok(invest) = Investment::from_str(&normalized_symbol) else {
            continue;
        };

        let exchange = Exchange::from_str(&invest.symbol[..2.min(invest.symbol.len())]);
        let quote = match exchange {
            Ok(Exchange::Sse | Exchange::SZse | Exchange::Bse) => parse_quote_cn(quote_str),
            Ok(Exchange::HKex) => parse_quote_hk(quote_str),
            Ok(Exchange::Nasdaq) => parse_quote_us(quote_str),
            _ => {
                debug!("unsupported exchange for symbol: {}", invest.symbol);
                continue;
            }
        };

        let Some(mut quote) = quote else {
            continue;
        };
        quote.symbol = invest.symbol;
        quotes.push(quote);
    }

    quotes
}

/// 解析 A 股行情
/// 格式: 中国平安,51.020,50.790,49.970,51.350,49.800,49.970,49.980,72935539,3688023391.000,...,2022-01-28,15:00:00,00,
fn parse_quote_cn(data: &str) -> Option<Quote> {
    let v: Vec<&str> = data.split(',').collect();
    if v.len() <= 31 || v[0].trim().is_empty() || v[30].trim().is_empty() || v[31].trim().is_empty() {
        return None;
    }

    let quote = Quote {
        symbol: String::new(),
        name: v[0].trim().to_string(),
        open: parse_quote_number(&v, 1)?,
        close: parse_quote_number(&v, 2)?,
        now: parse_quote_number(&v, 3)?,
        high: parse_quote_number(&v, 4)?,
        low: parse_quote_number(&v, 5)?,
        buy: parse_quote_number(&v, 6)?,
        sell: parse_quote_number(&v, 7)?,
        turnover: parse_quote_number(&v, 8)?,
        volume: parse_quote_number(&v, 9)?,
        date: v[30].trim().to_string(),
        time: v[31].trim().to_string(),
    };
    (quote.close > 0.0).then_some(quote)
}

/// 解析港股行情
/// 格式: TENCENT,腾讯控股,371.000,366.400,380.400,370.000,377.200,...,2022/03/29,16:00
fn parse_quote_hk(data: &str) -> Option<Quote> {
    let v: Vec<&str> = data.split(',').collect();
    if v.len() <= 18 || v[1].trim().is_empty() || v[17].trim().is_empty() || v[18].trim().is_empty() {
        return None;
    }

    let quote = Quote {
        symbol: String::new(),
        name: v[1].trim().to_string(),
        open: parse_quote_number(&v, 2)?,
        close: parse_quote_number(&v, 3)?,
        high: parse_quote_number(&v, 4)?,
        low: parse_quote_number(&v, 5)?,
        now: parse_quote_number(&v, 6)?,
        buy: parse_quote_number(&v, 9)?,
        sell: parse_quote_number(&v, 10)?,
        turnover: parse_quote_number(&v, 12)?,
        volume: parse_quote_number(&v, 11)?,
        date: v[17].trim().replace('/', "-"),
        time: v[18].trim().to_string(),
    };
    (quote.close > 0.0).then_some(quote)
}

/// 解析美股行情
/// 格式: 阿里巴巴,101.5500,-1.91,2022-04-12 09:30:09,...
fn parse_quote_us(data: &str) -> Option<Quote> {
    let v: Vec<&str> = data.split(',').collect();
    if v.len() <= 26 || v[0].trim().is_empty() {
        return None;
    }
    let (date, time) = v[3].trim().split_once(' ')?;
    if date.is_empty() || time.is_empty() {
        return None;
    }

    let quote = Quote {
        symbol: String::new(),
        name: v[0].trim().to_string(),
        now: parse_quote_number(&v, 1)?,
        close: parse_quote_number(&v, 26)?,
        open: parse_quote_number(&v, 5)?,
        high: parse_quote_number(&v, 6)?,
        low: parse_quote_number(&v, 7)?,
        // 新浪美股行情不提供可靠的最优买卖价，不以最高/最低价冒充。
        buy: 0.0,
        sell: 0.0,
        turnover: parse_quote_number(&v, 10)?,
        volume: match v.get(30) {
            Some(_) => parse_quote_number(&v, 30)?,
            None => 0.0,
        },
        date: date.to_string(),
        time: time.to_string(),
    };
    (quote.close > 0.0).then_some(quote)
}

fn parse_quote_number(fields: &[&str], index: usize) -> Option<f64> {
    fields.get(index)?.trim().parse().ok().filter(|value: &f64| value.is_finite() && *value >= 0.0)
}

fn parse_nonnegative_number(value: &str) -> f64 {
    value.trim().parse().ok().filter(|value: &f64| value.is_finite() && *value >= 0.0).unwrap_or(0.0)
}

/// 从字符串中提取数字
fn parse_number(s: &str) -> f64 {
    NUMBER_RE
        .find(s)
        .and_then(|value| value.as_str().replace(',', "").parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cn_payload(name: &str) -> String {
        let mut fields = vec!["0"; 32];
        fields[0] = name;
        fields[1] = "49.50";
        fields[2] = "49.53";
        fields[3] = "49.09";
        fields[4] = "50.00";
        fields[5] = "48.50";
        fields[6] = "49.08";
        fields[7] = "49.09";
        fields[8] = "53398530";
        fields[9] = "2613578800";
        fields[30] = "2026-07-14";
        fields[31] = "13:09:39";
        fields.join(",")
    }

    fn us_payload(name: &str) -> String {
        let mut fields = vec!["0"; 31];
        fields[0] = name;
        fields[1] = "112.35";
        fields[3] = "2026-07-14 09:48:52";
        fields[5] = "112.86";
        fields[6] = "114.20";
        fields[7] = "111.82";
        fields[10] = "9559900";
        fields[26] = "112.33";
        fields[30] = "1079000000";
        fields.join(",")
    }

    #[test]
    fn test_decode_response_respects_gb18030_and_rejects_invalid_bytes() {
        let expected = "扩展字符：𠮷";
        let (encoded, _, had_errors) = GB18030.encode(expected);
        assert!(!had_errors);
        assert_eq!(decode_response(&encoded, "text/plain; charset=GB18030", "https://hq.sinajs.cn").unwrap(), expected);
        assert!(decode_response(&[0x81], "text/plain; charset=gb18030", "https://hq.sinajs.cn").is_err());
        assert!(decode_response(b"ok", "text/plain; charset=unknown", "https://example.com").is_err());
    }

    #[test]
    fn test_format_quote_symbols_is_market_aware() {
        assert_eq!(
            format_quote_symbols("SH601318,sh601318,HK00700,HKHSI,$HKD,BRK.A").unwrap(),
            "sh601318,rt_hk00700,rt_hkHSI,gb_hkd,gb_brk$a"
        );
        assert!(format_quote_symbols("SH601318garbage").is_err());
        assert!(format_quote_symbols(",,").is_err());

        let too_many = (0..=MAX_QUOTE_SYMBOLS).map(|index| format!("SH{index:06}")).collect::<Vec<_>>().join(",");
        assert!(prepare_quote_request(&too_many).unwrap_err().to_string().contains("单次最多"));
    }

    #[test]
    fn test_parse_search_results_excludes_unsupported_bonds() {
        let content = r#"var suggestvalue="x,11,601318,sh601318,中国平安,,,x,1;x,15,019547,sh019547,国债,,,x,1";"#;
        let results = parse_search_results(content);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol, "SH601318");
        assert_eq!(results[0].market, Some(Market::Stock));
    }

    #[test]
    fn test_parse_snapshot_and_realtime_quotes() {
        let payload = cn_payload("中国平安");
        let snapshot = format!(r#"var hq_str_sh601318="{payload}";"#);
        let realtime = format!("sh601318={payload}");

        let snapshot_quotes = parse_quotes(&SNAPSHOT_QUOTE_RE, &snapshot);
        let realtime_quotes = parse_quotes(&REALTIME_QUOTE_RE, &realtime);

        assert_eq!(snapshot_quotes.len(), 1);
        assert_eq!(realtime_quotes.len(), 1);
        assert_eq!(snapshot_quotes[0].symbol, "SH601318");
        assert_eq!(snapshot_quotes[0].name, "中国平安");
        assert_eq!(snapshot_quotes[0].now, 49.09);
        assert_eq!(snapshot_quotes[0].traded_volume(), 53_398_530.0);
        assert_eq!(snapshot_quotes[0].traded_amount(), 2_613_578_800.0);
    }

    #[test]
    fn test_stock_profile_pe_uses_trailing_four_quarter_profit() {
        let quote = cn_payload("测试公司");
        let mut info = vec!["0"; 35];
        info[5] = "10";
        info[7] = "1000000";
        info[8] = "800000";
        info[13] = "25"; // 最近四季度净利润（亿元）
        info[18] = "5"; // 最近报告期净利润，不能用于 TTM PE
        info[34] = "测试行业";
        let content = format!("var hq=\"{quote}\";\nvar hq_i=\"{}\";", info.join(","));

        let profile = parse_stock_info(&content, Profile::default());
        let expected_market_cap = 49.09 * 1_000_000.0 * 10_000.0;

        assert_eq!(profile.market_cap, expected_market_cap);
        assert!((profile.pe_ttm - expected_market_cap / 25.0 / 100_000_000.0).abs() < f64::EPSILON);
        assert!((profile.pe_ttm - expected_market_cap / 5.0 / 100_000_000.0).abs() > 1.0);
    }

    #[test]
    fn test_parse_quote_rejects_missing_or_non_finite_financial_data() {
        assert!(parse_quote_cn("").is_none());

        let mut fields: Vec<_> = cn_payload("中国平安").split(',').map(str::to_owned).collect();
        fields[2] = "NaN".to_string();
        assert!(parse_quote_cn(&fields.join(",")).is_none());
        fields[2] = "49.53".to_string();
        fields[9] = "inf".to_string();
        assert!(parse_quote_cn(&fields.join(",")).is_none());
    }

    #[test]
    fn test_parse_hk_quote_uses_bid_and_ask_fields() {
        let mut fields = vec!["0"; 19];
        fields[0] = "TENCENT";
        fields[1] = "腾讯控股";
        fields[2] = "450.0";
        fields[3] = "451.0";
        fields[4] = "455.0";
        fields[5] = "445.0";
        fields[6] = "452.0";
        fields[9] = "451.8";
        fields[10] = "452.2";
        fields[11] = "1000";
        fields[12] = "452000";
        fields[17] = "2026/07/14";
        fields[18] = "13:00:00";

        let quote = parse_quote_hk(&fields.join(",")).unwrap();
        assert_eq!(quote.buy, 451.8);
        assert_eq!(quote.sell, 452.2);
        assert_eq!(quote.traded_volume(), 452_000.0);
        assert_eq!(quote.traded_amount(), 1_000.0);
        assert_eq!(quote.date, "2026-07-14");
    }

    #[test]
    fn test_parse_us_quote_does_not_fake_bid_and_ask() {
        let quote = parse_quote_us(&us_payload("Alibaba")).unwrap();
        assert_eq!(quote.buy, 0.0);
        assert_eq!(quote.sell, 0.0);
        assert_eq!(quote.traded_volume(), 9_559_900.0);
        assert_eq!(quote.traded_amount(), 1_079_000_000.0);
    }

    #[test]
    fn test_parse_dotted_us_symbol_and_short_index_payload() {
        let mut fields = vec!["0"; 30];
        fields[0] = "Dow Jones";
        fields[1] = "52498.64";
        fields[3] = "2026-07-14 04:20:23";
        fields[5] = "52676.53";
        fields[6] = "52846.51";
        fields[7] = "52351.12";
        fields[10] = "399286422";
        fields[26] = "52637.01";
        let index_payload = fields.join(",");

        let index_content = format!(r#"hq_str_gb_dji="{index_payload}";"#);
        let index_request = prepare_quote_request(".DJI").unwrap();
        let index_quotes = filter_requested_quotes(parse_quotes(&SNAPSHOT_QUOTE_RE, &index_content), &index_request);
        assert_eq!(index_quotes.len(), 1);
        assert_eq!(index_quotes[0].symbol, "$DJI");
        assert_eq!(index_quotes[0].volume, 0.0);

        let dotted_content = format!(r#"hq_str_gb_brk$a="{}";"#, us_payload("Berkshire"));
        let dotted_request = prepare_quote_request("BRK.A").unwrap();
        let dotted_quotes = filter_requested_quotes(parse_quotes(&SNAPSHOT_QUOTE_RE, &dotted_content), &dotted_request);
        assert_eq!(dotted_quotes.len(), 1);
        assert_eq!(dotted_quotes[0].symbol, "$BRK.A");
    }

    #[test]
    fn test_filter_requested_quotes_discards_unrequested_symbols() {
        let payload = cn_payload("测试");
        let content =
            format!(r#"hq_str_sh601318="{payload}";hq_str_sh601318="{payload}";hq_str_sz000001="{payload}";"#);
        let request = prepare_quote_request("SH601318").unwrap();
        let quotes = filter_requested_quotes(parse_quotes(&SNAPSHOT_QUOTE_RE, &content), &request);

        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].symbol, "SH601318");

        let reverse_request = prepare_quote_request("SZ000001,SH601318,SZ000001").unwrap();
        let ordered = filter_requested_quotes(parse_quotes(&SNAPSHOT_QUOTE_RE, &content), &reverse_request);
        assert_eq!(ordered.iter().map(|quote| quote.symbol.as_str()).collect::<Vec<_>>(), ["SZ000001", "SH601318"]);
    }

    #[test]
    fn test_complete_quotes_reports_partial_responses() {
        let payload = cn_payload("测试");
        let content = format!(r#"hq_str_sh601318="{payload}";"#);
        let quotes = parse_quotes(&SNAPSHOT_QUOTE_RE, &content);

        let request = prepare_quote_request("SH601318,SZ000001").unwrap();
        let error = ensure_complete_quotes(quotes, &request).unwrap_err();
        assert!(error.to_string().contains("SZ000001"));
    }

    #[test]
    fn test_provider_input_validation_rejects_short_unicode_and_controls() {
        assert!(validate_profile_symbol("").is_err());
        assert!(validate_profile_symbol("中").is_err());
        assert!(validate_profile_symbol("$BABA").is_err());
        assert!(validate_stock_code("中").is_err());
        assert!(validate_stock_code("12345").is_err());
        assert!(validate_search_query("\n").is_err());
        assert!(validate_search_query(&"a".repeat(MAX_SEARCH_QUERY_BYTES + 1)).is_err());
    }

    #[test]
    fn test_missing_symbols_preserves_request_order_and_uses_set_membership() {
        let request = prepare_quote_request("SZ000001,SH601318,BJ430047").unwrap();
        let received = HashSet::from(["SH601318".to_string()]);
        assert_eq!(missing_symbols(&request, &received), ["SZ000001", "BJ430047"]);
    }

    const FINANCIAL_FIXTURE: &str = r#"
    {
      "result": {
        "status": { "code": 0 },
        "data": {
          "report_list": {
            "20241231": {
              "data": [
                { "item_field": "BIZTOTINCO", "item_title": "其它", "item_value": "90", "item_tongbi": -0.10 },
                { "item_field": "UNEXPECTED_PROFIT", "item_title": "归属母公司股东的净利润", "item_value": "8", "item_tongbi": "0.02" },
                { "item_field": "NAPS", "item_title": "摊薄每股净资产_期末股数", "item_value": "5.1" },
                { "item_field": "CRPS", "item_title": "每股资本公积金", "item_value": "1.2" }
              ]
            },
            "20250331": {
              "data": [
                { "item_field": "UNEXPECTED_REVENUE", "item_title": "营业总收入", "item_value": "100.5", "item_tongbi": 0.125 },
                { "item_field": "PARENETP", "item_title": "其它", "item_value": "10.25", "item_tongbi": -0.05 },
                { "item_field": "NAPS", "item_title": "每股净资产", "item_value": "5.5" },
                { "item_field": "CRPS", "item_title": "其它", "item_value": "1.3" },
                { "item_field": "NPCUT", "item_title": "扣非净利润", "item_value": "9.5", "item_tongbi": 0.01 }
              ]
            }
          }
        }
      }
    }
    "#;

    #[test]
    fn test_parse_financial_json_fixture_maps_semantic_and_fallback_fields() {
        let financials = parse_financials(FINANCIAL_FIXTURE).unwrap();
        assert_eq!(financials.len(), 2);
        assert_eq!(financials[0].date, "2025-03-31");
        assert_eq!(financials[0].total_revenue, 100.5);
        assert_eq!(financials[0].total_revenue_rate, 12.5);
        assert_eq!(financials[0].net_profit, 10.25);
        assert_eq!(financials[0].net_profit_rate, -5.0);
        assert_eq!(financials[0].ps_net_assets, 5.5);
        assert_eq!(financials[0].ps_capital_reserve, 1.3);
        assert_eq!(financials[0].net_profit_after_nrgal, 9.5);
        assert_eq!(financials[1].date, "2024-12-31");
        assert_eq!(financials[1].total_revenue, 90.0);
        assert_eq!(financials[1].net_profit, 8.0);
    }

    #[test]
    fn test_parse_financial_json_rejects_null_empty_and_invalid_numbers() {
        let api_error = r#"{"result":{"status":{"code":500,"msg":"upstream failed"},"data":null}}"#;
        assert!(parse_financials(api_error).unwrap_err().to_string().contains("500"));

        let null_data = r#"{"result":{"status":{"code":0},"data":null}}"#;
        assert!(parse_financials(null_data).unwrap_err().to_string().contains("未提供数据"));

        let empty = r#"{"result":{"status":{"code":0},"data":{"report_list":{}}}}"#;
        assert!(parse_financials(empty).unwrap_err().to_string().contains("空报表"));

        let malformed = FINANCIAL_FIXTURE.replacen("\"100.5\"", "\"NaN\"", 1);
        let error = parse_financials(&malformed).unwrap_err();
        assert!(format!("{error:#}").contains("有限数值"));

        assert_eq!(format_report_date("20240229").unwrap(), "2024-02-29");
        assert!(format_report_date("20250229").is_err());
        assert!(format_report_date("20250431").is_err());
    }

    #[test]
    fn test_parsers_keep_last_complete_record_at_eof() {
        let mut structure_rows = String::new();
        for (date, holders, average) in [("2025-03-31", "100", "10"), ("2025-06-30", "184,012户", "20")] {
            for row in 0..17 {
                let value = match row {
                    0 => date,
                    3 => holders,
                    4 => average,
                    _ => "",
                };
                structure_rows.push_str(&format!("<tr><td>{value}</td></tr>"));
            }
        }
        let structures = parse_structures(&format!("<table id=\"Table1\"><tbody>{structure_rows}</tbody></table>"));
        assert_eq!(structures.len(), 2);
        assert_eq!(structures[1].date, "2025-06-30");
        assert_eq!(structures[1].holders_num, 184_012.0);

        let dividends = parse_dividends(
            r#"<table id="sharebonus_1">
                <tr><td>2025-01-01</td><td>1</td><td>2</td><td>3</td><td></td><td>2025-01-02</td><td>2025-01-03</td><td></td><td></td></tr>
                <tr><td><span>2026-01-01</span></td><td>4</td><td>5</td><td><strong>6</strong></td><td></td><td>2026-01-02</td><td>2026-01-03</td><td></td><td></td></tr>
            </table>"#,
        );
        assert_eq!(dividends.len(), 2);
        assert_eq!(dividends[1].date, "2026-01-01");
        assert_eq!(dividends[1].money, 6.0);

        let presses = parse_presses(
            r#"<div class="datelist"><ul>2025-01-01<li><a href="/a"><span>公告 A</span></a></li><i></i>2026-01-01<li href="/b"><strong>公告 B</strong></li><i></i></ul></div>"#,
        );
        assert_eq!(presses.len(), 2);
        assert_eq!(presses[1].date, "2026-01-01");
        assert_eq!(presses[1].title, "公告 B");
        assert_eq!(presses[0].url, "https://vip.stock.finance.sina.com.cn/a");
        assert_eq!(presses[1].url, "https://vip.stock.finance.sina.com.cn/b");

        assert_eq!(resolve_press_url(Some("https://example.com/report")), "https://example.com/report");
        assert!(resolve_press_url(Some("javascript:alert(1)")).is_empty());
        assert_eq!(parse_number("-1,234.50 万股"), -1234.5);
    }
}
