use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    io::{self, IsTerminal, Write, stdout},
};

use crossterm::{QueueableCommand, cursor, style::Stylize, terminal, terminal::ClearType};
use unicode_width::UnicodeWidthStr;

use crate::invest::quote::Quote;

const SYMBOL_WIDTH: usize = 8;
const WIDE_NAME_WIDTH: usize = 10;
const MEDIUM_NAME_WIDTH: usize = 8;
const PRICE_WIDTH: usize = 10;
const RATE_WIDTH: usize = 12;
const AMOUNT_WIDTH: usize = 10;

/// 格式化数字为易读形式（万/亿）
pub fn format_number(num: &f64) -> String {
    const YI: f64 = 100_000_000.0;
    const WAN: f64 = 10_000.0;

    let formatted = match *num {
        n if !n.is_finite() || n == 0.0 => " - ".to_string(),
        n if n.abs() >= YI => format!("{:.2}亿", n / YI),
        n => format!("{:.2}万", n / WAN),
    };
    if UnicodeWidthStr::width(formatted.as_str()) <= 16 || !num.is_finite() {
        formatted
    } else if num.abs() >= YI {
        format!("{:.2e}亿", num / YI)
    } else {
        format!("{:.2e}万", num / WAN)
    }
}

/// 计算涨跌幅
#[inline]
fn calculate_rate(now: f64, close: f64) -> Option<f64> {
    if !now.is_finite() || !close.is_finite() || now < 0.0 || close <= 0.0 {
        None
    } else {
        let rate = (now / close - 1.0) * 100.0;
        rate.is_finite().then_some(rate)
    }
}

/// 判断是否为港股指数
#[inline]
fn is_hk_index(symbol: &str) -> bool {
    symbol.strip_prefix("HK").is_some_and(|code| code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()))
}

/// 移除会改变终端状态或破坏单行布局的控制字符。
fn is_unsafe_terminal_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{feff}'
        )
}

pub fn sanitize_text(s: &str) -> Cow<'_, str> {
    if s.chars().all(|c| !is_unsafe_terminal_char(c)) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.chars().map(|c| if is_unsafe_terminal_char(c) { ' ' } else { c }).collect())
    }
}

/// 按终端显示宽度左侧填充。
fn pad_left(s: &str, min_width: usize) -> String {
    let width = UnicodeWidthStr::width(s);
    if width > min_width { "#".repeat(min_width) } else { format!("{}{}", " ".repeat(min_width - width), s) }
}

fn format_decimal(value: f64, width: usize) -> String {
    let valid = value.is_finite() && value >= 0.0;
    let mut formatted = if valid { format!("{value:.2}") } else { "-".to_string() };
    if valid && UnicodeWidthStr::width(formatted.as_str()) > width {
        formatted = format!("{value:.2e}");
    }
    pad_left(&formatted, width)
}

fn format_amount(value: f64) -> String {
    let mut formatted = format_number(&value);
    if UnicodeWidthStr::width(formatted.as_str()) > AMOUNT_WIDTH {
        formatted = if value.is_finite() { format!("{value:.2e}") } else { "-".to_string() };
    }
    pad_left(&formatted, AMOUNT_WIDTH)
}

/// 将文本裁剪或填充到指定终端显示宽度。
pub(crate) fn fit_text(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let max_chars = max_width.saturating_mul(2).max(1);
    let sanitized: String =
        s.chars().take(max_chars).map(|c| if is_unsafe_terminal_char(c) { ' ' } else { c }).collect();

    // Emoji 变体选择符和修饰符会改变前一个字符的宽度，因此不能累加单字符宽度。
    // 扫描所有前缀并保留最长的合法前缀，也允许后续选择符把先前过宽的前缀缩窄。
    let mut best_end = 0;
    let mut width = 0;
    for (index, (byte_index, c)) in sanitized.char_indices().enumerate() {
        let end = byte_index + c.len_utf8();
        let prefix_width = UnicodeWidthStr::width(&sanitized[..end]);
        let final_chars = index + 1 + max_width.saturating_sub(prefix_width);
        if prefix_width <= max_width && final_chars <= max_chars {
            best_end = end;
            width = prefix_width;
        }
    }

    let mut result = sanitized[..best_end].to_string();
    while width < max_width {
        result.push(' ');
        width += 1;
    }
    result
}

/// 格式化涨跌幅（带箭头和颜色）
fn format_rate(rate: Option<f64>, color: bool) -> String {
    let Some(rate) = rate else {
        return pad_left("-", RATE_WIDTH);
    };
    // 正负号和颜色已表达方向；避免终端对三角箭头的 East Asian Ambiguous 宽度解释不一致。
    let suffix = "%";
    let number_width = RATE_WIDTH.saturating_sub(UnicodeWidthStr::width(suffix));
    let mut number = format!("{rate:+.2}");
    if UnicodeWidthStr::width(number.as_str()) > number_width {
        number = format!("{rate:+.2e}");
    }
    let rate_str = format!("{}{}", pad_left(&number, number_width), suffix);
    if !color {
        rate_str
    } else if rate > 0.0 {
        rate_str.red().bold().to_string()
    } else if rate < 0.0 {
        rate_str.green().bold().to_string()
    } else {
        rate_str.dark_grey().to_string()
    }
}

/// 格式化价格（带颜色）
fn format_price(price: f64, rate: Option<f64>, color: bool) -> String {
    let price_str = format_decimal(price, PRICE_WIDTH);
    if !color {
        price_str
    } else if rate.is_some_and(|rate| rate > 0.0) {
        price_str.red().bold().to_string()
    } else if rate.is_some_and(|rate| rate < 0.0) {
        price_str.green().bold().to_string()
    } else {
        price_str.dark_grey().to_string()
    }
}

/// 显示模式
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DisplayMode {
    /// 宽屏 (>=160): 完整信息
    Wide,
    /// 中等 (80-159): 紧凑信息
    Medium,
    /// 窄屏 (<80): 最小信息
    Narrow,
}

/// 根据终端宽度决定显示模式。
fn display_mode(width: u16) -> DisplayMode {
    match width {
        w if w >= 160 => DisplayMode::Wide,
        w if w >= 80 => DisplayMode::Medium,
        _ => DisplayMode::Narrow,
    }
}

/// 每种布局都由定宽字段组成；集中维护宽度可避免实时热路径为安全检查重复格式化。
fn quote_line_width(mode: DisplayMode) -> usize {
    match mode {
        DisplayMode::Wide => 157,
        DisplayMode::Medium => 61,
        DisplayMode::Narrow => 32,
    }
}

/// 格式化行情为字符串（根据终端宽度自适应）
fn format_quote_line(quote: &Quote, mode: DisplayMode, color: bool) -> String {
    let traded_volume = quote.traded_volume();
    let traded_amount = if is_hk_index(&quote.symbol) { quote.traded_amount() * 1000.0 } else { quote.traded_amount() };
    let rate = calculate_rate(quote.now, quote.close);
    let symbol = fit_text(&quote.symbol, SYMBOL_WIDTH);

    match mode {
        DisplayMode::Wide => {
            let traded_volume = format_amount(traded_volume);
            let traded_amount = format_amount(traded_amount);
            format!(
                "{} {} │ {} │ {} {} │ {} │ 昨:{} 开:{} 高:{} 低:{} │ 量:{} 额:{}",
                fit_text(&quote.date, 10),
                fit_text(&quote.time, 8),
                symbol,
                fit_text(&quote.name, WIDE_NAME_WIDTH),
                format_price(quote.now, rate, color),
                format_rate(rate, color),
                format_decimal(quote.close, PRICE_WIDTH),
                format_decimal(quote.open, PRICE_WIDTH),
                format_decimal(quote.high, PRICE_WIDTH),
                format_decimal(quote.low, PRICE_WIDTH),
                traded_volume,
                traded_amount,
            )
        }
        DisplayMode::Medium => {
            format!(
                "{} │ {} {} │ {} │ 额:{}",
                symbol,
                fit_text(&quote.name, MEDIUM_NAME_WIDTH),
                format_price(quote.now, rate, color),
                format_rate(rate, color),
                format_amount(traded_amount),
            )
        }
        DisplayMode::Narrow => {
            format!("{} {} {}", symbol, format_price(quote.now, rate, color), format_rate(rate, color))
        }
    }
}

/// 输出行情报价
pub fn print_quote(quote: &Quote) -> io::Result<()> {
    print_quotes(std::slice::from_ref(quote))
}

/// 批量输出行情；非终端输出使用稳定、无颜色的完整格式。
pub fn print_quotes(quotes: &[Quote]) -> io::Result<()> {
    let stdout = stdout();
    let interactive = stdout.is_terminal();
    let mode = if interactive {
        terminal::size().map(|(width, _)| display_mode(width)).unwrap_or(DisplayMode::Medium)
    } else {
        DisplayMode::Wide
    };
    let mut stdout = stdout.lock();

    for quote in quotes {
        writeln!(stdout, "{}", format_quote_line(quote, mode, interactive))?;
    }
    stdout.flush()
}

/// 实时行情输出管理器
pub struct RealtimeQuotePrinter {
    /// 股票行号映射（symbol -> line_index，0-indexed）
    line_map: HashMap<String, usize>,
    /// 已绘制的最终行，用于跳过服务端高频推送的完全相同帧。
    line_cache: HashMap<String, String>,
    /// 首次绘制时的终端尺寸；尺寸变化后不再依赖旧光标布局。
    dashboard_size: Option<(u16, u16)>,
    /// 窄终端、重定向或空间不足时永久退化为追加输出。
    dashboard_disabled: bool,
}

impl RealtimeQuotePrinter {
    pub fn new() -> Self {
        Self { line_map: HashMap::new(), line_cache: HashMap::new(), dashboard_size: None, dashboard_disabled: false }
    }

    /// 处理实时行情数据
    pub fn print(&mut self, quotes: &[Quote], multiline: bool) -> io::Result<()> {
        let stdout = stdout();
        let terminal_size = if stdout.is_terminal() { terminal::size().ok() } else { None };
        let mut stdout = stdout.lock();
        self.print_to(&mut stdout, quotes, multiline, terminal_size)
    }

    /// 保持光标位于行情块下一行，只用相对移动更新已有行，避免查询绝对光标位置。
    fn print_to<W: Write>(
        &mut self,
        writer: &mut W,
        quotes: &[Quote],
        multiline: bool,
        terminal_size: Option<(u16, u16)>,
    ) -> io::Result<()> {
        if quotes.is_empty() {
            return Ok(());
        }

        let interactive = terminal_size.is_some();
        let mode = terminal_size.map(|(width, _)| display_mode(width)).unwrap_or(DisplayMode::Wide);

        // 多行模式用于保留单证券的更新历史。
        if multiline {
            for quote in quotes {
                writeln!(writer, "{}", format_quote_line(quote, mode, interactive))?;
            }
            return writer.flush();
        }

        if !self.dashboard_disabled {
            self.dashboard_disabled = !self.dashboard_is_safe(quotes, terminal_size, mode);
            if !self.dashboard_disabled && self.dashboard_size.is_none() {
                self.dashboard_size = terminal_size;
            }
        }

        // 管道/文件输出及不满足单行寻址条件的终端只做追加，不输出 ANSI 光标指令。
        if self.dashboard_disabled {
            for quote in quotes {
                writeln!(writer, "{}", format_quote_line(quote, mode, interactive))?;
            }
            return writer.flush();
        }

        for quote in quotes {
            let line = format_quote_line(quote, mode, true);

            if let Some(&line_idx) = self.line_map.get(&quote.symbol) {
                if self.line_cache.get(&quote.symbol).is_some_and(|previous| previous == &line) {
                    continue;
                }
                let distance = self.line_map.len() - line_idx;
                let distance =
                    u16::try_from(distance).map_err(|_| io::Error::other("实时行情行数超过终端光标可表示范围"))?;

                writer.queue(cursor::MoveToPreviousLine(distance))?;
                writer.queue(terminal::Clear(ClearType::CurrentLine))?;
                write!(writer, "{}", line)?;
                writer.queue(cursor::MoveToNextLine(distance))?;
                self.line_cache.insert(quote.symbol.clone(), line);
            } else {
                if self.line_map.len() == usize::from(u16::MAX) {
                    return Err(io::Error::other("实时行情行数超过终端光标可表示范围"));
                }

                writer.queue(terminal::Clear(ClearType::CurrentLine))?;
                writeln!(writer, "{}", line)?;
                self.line_map.insert(quote.symbol.clone(), self.line_map.len());
                self.line_cache.insert(quote.symbol.clone(), line);
            }
        }

        writer.flush()
    }

    fn dashboard_is_safe(&self, quotes: &[Quote], terminal_size: Option<(u16, u16)>, mode: DisplayMode) -> bool {
        let Some((width, height)) = terminal_size else {
            return false;
        };
        if self.dashboard_size.is_some_and(|size| size != (width, height)) {
            return false;
        }

        let mut unseen = HashSet::new();
        for quote in quotes {
            if !self.line_map.contains_key(&quote.symbol) {
                unseen.insert(quote.symbol.as_str());
            }
        }
        if quote_line_width(mode) > usize::from(width) {
            return false;
        }

        // 预留一行作为稳定光标锚点；滚出可视区域后相对寻址不再可靠。
        self.line_map.len() + unseen.len() < usize::from(height)
    }
}

impl Default for RealtimeQuotePrinter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== format_number 测试 ====================

    #[test]
    fn test_format_number_yi() {
        // 大于等于1亿显示为亿
        assert_eq!(format_number(&100_000_000.0), "1.00亿");
        assert_eq!(format_number(&123_456_789.0), "1.23亿");
        assert_eq!(format_number(&500_000_000_000.0), "5000.00亿");
    }

    #[test]
    fn test_format_number_wan() {
        // 小于1亿显示为万
        assert_eq!(format_number(&10_000.0), "1.00万");
        assert_eq!(format_number(&50_000_000.0), "5000.00万");
        assert_eq!(format_number(&12345.67), "1.23万");
    }

    #[test]
    fn test_format_number_zero() {
        assert_eq!(format_number(&0.0), " - ");
        assert_eq!(format_number(&f64::NAN), " - ");
        assert_eq!(format_number(&f64::INFINITY), " - ");
    }

    #[test]
    fn test_format_number_small() {
        // 小于1万也显示为万
        assert_eq!(format_number(&100.0), "0.01万");
        assert_eq!(format_number(&1.0), "0.00万");
        assert_eq!(format_number(&-123_456_789.0), "-1.23亿");
    }

    // ==================== calculate_rate 测试 ====================

    #[test]
    fn test_calculate_rate() {
        // 涨10%
        assert!((calculate_rate(110.0, 100.0).unwrap() - 10.0).abs() < 0.001);

        // 跌10%
        assert!((calculate_rate(90.0, 100.0).unwrap() - (-10.0)).abs() < 0.001);

        // 平盘
        assert!((calculate_rate(100.0, 100.0).unwrap() - 0.0).abs() < 0.001);

        // 昨收为0的情况
        assert_eq!(calculate_rate(100.0, 0.0), None);
        assert_eq!(calculate_rate(f64::NAN, 100.0), None);
        assert_eq!(calculate_rate(100.0, f64::INFINITY), None);
        assert_eq!(calculate_rate(f64::MAX, 0.01), None);
    }

    // ==================== is_hk_index 测试 ====================

    #[test]
    fn test_is_hk_index() {
        // 港股指数（3个大写字母）
        assert!(is_hk_index("HKHSI"));
        assert!(is_hk_index("HKHCE"));

        // 港股普通股票（5位数字）
        assert!(!is_hk_index("HK00700"));
        assert!(!is_hk_index("HK09988"));

        // 其他市场
        assert!(!is_hk_index("SH601318"));
        assert!(!is_hk_index("$BABA"));
        assert!(!is_hk_index("XHKHSI"));
        assert!(!is_hk_index("HKHSIX"));
    }

    #[test]
    fn test_fit_text_uses_terminal_width_and_sanitizes_controls() {
        assert_eq!(fit_text("中国A", 5), "中国A");
        assert_eq!(fit_text("中国A", 4), "中国");
        assert_eq!(fit_text("中国A", 6), "中国A ");
        assert_eq!(fit_text("A\nB", 3), "A B");
        assert_eq!(fit_text("A\u{202e}B", 3), "A B");
        assert_eq!(fit_text("A\u{2028}B", 3), "A B");

        for input in ["#\u{fe0f}", "♈\u{fe0e}", "👪🏻"] {
            let fitted = fit_text(input, 4);
            assert_eq!(UnicodeWidthStr::width(fitted.as_str()), 4, "{input:?}");
        }

        let zero_width_abuse = format!("A{}", "\u{301}".repeat(1_000));
        let fitted = fit_text(&zero_width_abuse, 3);
        assert_eq!(UnicodeWidthStr::width(fitted.as_str()), 3);
        assert!(fitted.chars().count() <= 6);
    }

    fn sample_quote(symbol: &str) -> Quote {
        Quote {
            symbol: symbol.to_string(),
            name: "中国\u{1b}[2J平安".to_string(),
            now: 11.0,
            close: 10.0,
            open: 10.0,
            high: 12.0,
            low: 9.0,
            turnover: 12_345.0,
            volume: 123_456.0,
            date: "2026-07-14".to_string(),
            time: "10:00:00".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_format_quote_line_has_no_untrusted_control_characters() {
        let line = format_quote_line(&sample_quote("SH601318"), DisplayMode::Medium, false);
        assert!(!line.contains('\u{1b}'));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn test_display_mode_boundaries() {
        assert_eq!(display_mode(79), DisplayMode::Narrow);
        assert_eq!(display_mode(80), DisplayMode::Medium);
        assert_eq!(display_mode(159), DisplayMode::Medium);
        assert_eq!(display_mode(160), DisplayMode::Wide);
    }

    #[test]
    fn test_wide_quote_line_keeps_snapshot_fields() {
        let line = format_quote_line(&sample_quote("SH601318"), DisplayMode::Wide, false);
        assert!(line.contains("2026-07-14 10:00:00"));
        assert!(line.contains("昨:"));
        assert!(line.contains("开:"));
        assert!(line.contains("量:"));
        assert!(line.contains("额:"));
    }

    #[test]
    fn test_quote_columns_stay_aligned_for_wide_values_and_untrusted_text() {
        let regular = sample_quote("SH601318");
        let mut extreme = sample_quote("$BRK.A");
        extreme.name = "超长\n名称\u{301}\u{301}\u{301}ABCDEFGHIJK".to_string();
        extreme.now = f64::MAX;
        extreme.close = 0.01;
        extreme.open = f64::MAX;
        extreme.high = f64::MAX;
        extreme.low = 0.0;
        extreme.turnover = f64::MAX;
        extreme.volume = f64::MAX;
        extreme.date = "2026-07-14\nINJECT".to_string();

        for (mode, max_width) in [(DisplayMode::Wide, 160), (DisplayMode::Medium, 80), (DisplayMode::Narrow, 79)] {
            let regular_line = format_quote_line(&regular, mode, false);
            let extreme_line = format_quote_line(&extreme, mode, false);
            let regular_width = UnicodeWidthStr::width(regular_line.as_str());
            let extreme_width = UnicodeWidthStr::width(extreme_line.as_str());

            assert_eq!(regular_width, extreme_width);
            assert_eq!(regular_width, quote_line_width(mode));
            assert!(regular_width <= max_width);
            assert!(!extreme_line.contains(['\n', '\r', '\u{2028}', '\u{2029}']));
            assert!(!extreme_line.contains("+0.00%"));
        }
    }

    // ==================== RealtimeQuotePrinter 测试 ====================

    #[test]
    fn test_realtime_quote_printer_new() {
        let printer = RealtimeQuotePrinter::new();
        assert!(printer.line_map.is_empty());
        assert!(printer.line_cache.is_empty());
        assert!(printer.dashboard_size.is_none());
        assert!(!printer.dashboard_disabled);
    }

    #[test]
    fn test_realtime_quote_printer_default() {
        let printer = RealtimeQuotePrinter::default();
        assert!(printer.line_map.is_empty());
    }

    #[test]
    fn test_realtime_quote_printer_updates_with_relative_cursor_movement() {
        let mut printer = RealtimeQuotePrinter::new();
        let quote = sample_quote("SH601318");
        let mut output = Vec::new();

        printer.print_to(&mut output, std::slice::from_ref(&quote), false, Some((79, 24))).unwrap();
        let mut updated = quote;
        updated.now = 11.1;
        printer.print_to(&mut output, std::slice::from_ref(&updated), false, Some((79, 24))).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\u{1b}[1F"));
        assert!(output.contains("\u{1b}[1E"));
        assert_eq!(printer.line_map.len(), 1);
    }

    #[test]
    fn test_realtime_quote_printer_skips_identical_dashboard_frames() {
        let mut printer = RealtimeQuotePrinter::new();
        let quote = sample_quote("SH601318");
        let mut output = Vec::new();

        printer.print_to(&mut output, std::slice::from_ref(&quote), false, Some((79, 24))).unwrap();
        output.clear();
        printer.print_to(&mut output, std::slice::from_ref(&quote), false, Some((79, 24))).unwrap();

        assert!(output.is_empty());
    }

    #[test]
    fn test_realtime_quote_printer_moves_exactly_across_multiple_lines() {
        let mut printer = RealtimeQuotePrinter::new();
        let first = sample_quote("SH601318");
        let second = sample_quote("SZ000001");
        let mut output = Vec::new();

        printer.print_to(&mut output, &[first, second], false, Some((79, 24))).unwrap();
        output.clear();
        let mut updated = sample_quote("SH601318");
        updated.now = 11.1;
        printer.print_to(&mut output, &[updated], false, Some((79, 24))).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("\u{1b}[2F\u{1b}[2K"));
        assert!(output.ends_with("\u{1b}[2E"));
        assert!(!output.contains('\n'));
    }

    #[test]
    fn test_realtime_quote_printer_downgrades_after_resize() {
        let mut printer = RealtimeQuotePrinter::new();
        let quote = sample_quote("SH601318");
        let mut output = Vec::new();

        printer.print_to(&mut output, std::slice::from_ref(&quote), false, Some((79, 24))).unwrap();
        output.clear();
        printer.print_to(&mut output, std::slice::from_ref(&quote), false, Some((80, 24))).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("\u{1b}[1F"));
        assert!(output.ends_with('\n'));
        assert!(printer.dashboard_disabled);
    }

    #[test]
    fn test_realtime_quote_printer_appends_in_multiline_mode() {
        let mut printer = RealtimeQuotePrinter::new();
        let quote = sample_quote("SH601318");
        let mut output = Vec::new();

        printer.print_to(&mut output, std::slice::from_ref(&quote), true, Some((79, 24))).unwrap();
        printer.print_to(&mut output, std::slice::from_ref(&quote), true, Some((79, 24))).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.lines().count(), 2);
        assert!(!output.contains("\u{1b}[2K"));
        assert!(printer.line_map.is_empty());
    }

    #[test]
    fn test_realtime_quote_printer_uses_plain_append_output_when_redirected() {
        let mut printer = RealtimeQuotePrinter::new();
        let quote = sample_quote("SH601318");
        let mut output = Vec::new();

        printer.print_to(&mut output, std::slice::from_ref(&quote), false, None).unwrap();
        printer.print_to(&mut output, std::slice::from_ref(&quote), false, None).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.lines().count(), 2);
        assert!(!output.contains('\u{1b}'));
        assert!(printer.line_map.is_empty());
        assert!(printer.dashboard_disabled);
    }

    #[test]
    fn test_realtime_quote_printer_disables_dashboard_when_line_wraps() {
        let mut printer = RealtimeQuotePrinter::new();
        let quote = sample_quote("SH601318");
        let mut output = Vec::new();

        printer.print_to(&mut output, std::slice::from_ref(&quote), false, Some((10, 24))).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("\u{1b}[2K"));
        assert!(printer.dashboard_disabled);
        assert!(printer.line_map.is_empty());
    }

    #[test]
    fn test_realtime_quote_printer_disables_dashboard_when_height_is_insufficient() {
        let mut printer = RealtimeQuotePrinter::new();
        let first = sample_quote("SH601318");
        let second = sample_quote("SZ000001");
        let mut output = Vec::new();

        printer.print_to(&mut output, &[first, second], false, Some((79, 2))).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("\u{1b}[2K"));
        assert!(printer.dashboard_disabled);
        assert!(printer.line_map.is_empty());
    }
}
