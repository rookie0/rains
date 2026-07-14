//! 股票信息查询命令

use std::io::{self, IsTerminal, Write, stdout};

use anyhow::{Result, bail};
use crossterm::style::Stylize;

use crate::{
    commands::{is_supported_exchange, validate_symbol},
    format::{fit_text, format_number},
    invest::stock::{Dividend, Financial, Press, Profile, Structure},
    provider::Provider,
};

/// 信息查询选项
pub struct InfoOptions {
    pub all: bool,
    pub financials: bool,
    pub structure: bool,
    pub dividends: bool,
    pub presses: bool,
}

/// 处理信息查询命令
pub async fn handle_info<P: Provider>(provider: &P, symbol: &str, options: InfoOptions) -> Result<()> {
    let invest = validate_symbol(provider, symbol).await?;

    if !is_supported_exchange(&invest.exchange) {
        bail!("当前仅支持沪深北证股票信息查询");
    }

    let symbol = &invest.symbol;
    let code = &symbol[2..];
    let include_financials = options.all || options.financials;
    let include_structure = options.all || options.structure;
    let include_dividends = options.all || options.dividends;
    let include_presses = options.all || options.presses;

    // 请求相互独立，并发获取后再统一渲染；任一请求失败时不会留下半份报告。
    let (profile, financials, structures, dividends, presses) = tokio::try_join!(
        provider.profile(symbol),
        async { if include_financials { provider.financials(code).await.map(Some) } else { Ok(None) } },
        async { if include_structure { provider.structures(code).await.map(Some) } else { Ok(None) } },
        async { if include_dividends { provider.dividends(code).await.map(Some) } else { Ok(None) } },
        async { if include_presses { provider.presses(code).await.map(Some) } else { Ok(None) } },
    )?;

    if financials.as_ref().is_some_and(Vec::is_empty) {
        bail!("财务指标接口未返回有效数据");
    }
    if structures.as_ref().is_some_and(Vec::is_empty) {
        bail!("股东结构接口未返回有效数据");
    }
    if dividends.as_ref().is_some_and(Vec::is_empty) {
        bail!("分红送配接口未返回有效数据");
    }
    if presses.as_ref().is_some_and(Vec::is_empty) {
        bail!("公告接口未返回有效数据");
    }

    let stdout = stdout();
    let styled = stdout.is_terminal();
    let mut writer = stdout.lock();
    write_profile(&mut writer, symbol, &profile, styled)?;
    if let Some(financials) = financials {
        write_financials(&mut writer, &financials, styled)?;
    }
    if let Some(structures) = structures {
        write_structures(&mut writer, &structures, styled)?;
    }
    if let Some(dividends) = dividends {
        write_dividends(&mut writer, &dividends, styled)?;
    }
    if let Some(presses) = presses {
        write_presses(&mut writer, &presses, styled)?;
    }
    writer.flush()?;

    Ok(())
}

fn heading(text: &str, styled: bool) -> String {
    if styled { text.bold().to_string() } else { text.to_string() }
}

fn table_row(cells: &[(&str, usize)]) -> String {
    let mut row = String::new();
    for (index, (value, width)) in cells.iter().enumerate() {
        if index > 0 {
            row.push_str(" │ ");
        }
        row.push_str(&fit_text(value, *width));
    }
    row
}

fn bounded_text(value: &str) -> String {
    fit_text(value, 512).trim_end().to_string()
}

fn write_field<W: Write>(writer: &mut W, label: &str, value: impl AsRef<str>) -> io::Result<()> {
    writeln!(writer, "{} {}", fit_text(label, 10), bounded_text(value.as_ref()))
}

fn write_trusted_field<W: Write>(writer: &mut W, label: &str, value: &str) -> io::Result<()> {
    writeln!(writer, "{} {}", fit_text(label, 10), value)
}

fn decimal(value: f64, precision: usize) -> String {
    if !value.is_finite() {
        return "-".to_string();
    }
    let fixed = format!("{value:.precision$}");
    if fixed.len() <= 16 { fixed } else { format!("{value:.4e}") }
}

fn positive_decimal(value: f64, precision: usize) -> String {
    if value > 0.0 { decimal(value, precision) } else { "-".to_string() }
}

fn write_profile<W: Write>(writer: &mut W, symbol: &str, profile: &Profile, styled: bool) -> io::Result<()> {
    let website = bounded_text(&profile.website);
    let website = if styled { website.as_str().underlined().to_string() } else { website };
    writeln!(writer, "{}", heading("基本信息", styled))?;
    write_field(writer, "证券代码", symbol)?;
    write_field(writer, "简称历史", &profile.used_name)?;
    write_field(writer, "公司名称", &profile.name)?;
    write_field(writer, "上市日期", &profile.listing_date)?;
    write_field(writer, "发行价格", positive_decimal(profile.listing_price, 2))?;
    write_field(writer, "行业分类", &profile.category)?;
    write_field(writer, "主营业务", &profile.business)?;
    write_field(writer, "办公地址", &profile.business_address)?;
    write_trusted_field(writer, "公司网址", &website)?;
    write_field(writer, "当前价格", positive_decimal(profile.price, 2))?;
    write_field(writer, "市净率PB", positive_decimal(profile.pb, 2))?;
    write_field(writer, "市盈率TTM", positive_decimal(profile.pe_ttm, 2))?;
    write_field(writer, "总市值", format_number(&profile.market_cap))?;
    write_field(writer, "流通市值", format_number(&profile.traded_market_cap))?;
    Ok(())
}

fn write_financials<W: Write>(writer: &mut W, financials: &[Financial], styled: bool) -> io::Result<()> {
    writeln!(writer, "\n{}", heading("财务指标", styled))?;
    writeln!(
        writer,
        "{}",
        table_row(&[("截止日期", 10), ("总营收", 12), ("净利润", 12), ("每股净资产", 12), ("每股资本公积", 14)])
    )?;
    for f in financials {
        let revenue = format_number(&f.total_revenue);
        let profit = format_number(&f.net_profit);
        let net_assets = decimal(f.ps_net_assets, 4);
        let capital_reserve = decimal(f.ps_capital_reserve, 4);
        writeln!(
            writer,
            "{}",
            table_row(&[(&f.date, 10), (&revenue, 12), (&profit, 12), (&net_assets, 12), (&capital_reserve, 14),])
        )?;
    }
    Ok(())
}

fn write_structures<W: Write>(writer: &mut W, structures: &[Structure], styled: bool) -> io::Result<()> {
    let Some(first) = structures.first() else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "股东结构为空"));
    };
    writeln!(writer, "\n{}", heading("股东结构", styled))?;

    writeln!(writer, "{}", table_row(&[("截止日期", 12), ("股东户数", 14), ("平均持股", 14)]))?;
    for structure in structures {
        let holders = format_number(&structure.holders_num);
        let shares = format_number(&structure.shares_avg);
        writeln!(writer, "{}", table_row(&[(&structure.date, 12), (&holders, 14), (&shares, 14)]))?;
    }

    writeln!(writer, "\n{}", heading("十大股东", styled))?;
    writeln!(writer, "{}", table_row(&[("序号", 4), ("股东名称", 30), ("持股比例", 10), ("持股数", 14)]))?;
    for (i, h) in first.holders_ten.iter().enumerate() {
        let index = (i + 1).to_string();
        let percent =
            if h.percent.is_finite() && h.percent >= 0.0 { format!("{}%", decimal(h.percent, 2)) } else { "-".into() };
        let shares = format_number(&h.shares);
        writeln!(writer, "{}", table_row(&[(&index, 4), (&h.name, 30), (&percent, 10), (&shares, 14)]))?;
    }
    Ok(())
}

fn write_dividends<W: Write>(writer: &mut W, dividends: &[Dividend], styled: bool) -> io::Result<()> {
    writeln!(writer, "\n{}", heading("分红送配", styled))?;
    writeln!(writer, "{}", table_row(&[("公告日期", 10), ("分红送配", 28), ("除权除息日", 10), ("股权登记日", 10)]))?;

    for d in dividends {
        let mut parts = Vec::new();
        let values = [d.shares_dividend, d.shares_into, d.money];
        let invalid = values.iter().any(|value| !value.is_finite() || *value < 0.0);
        if !invalid && d.shares_dividend > 0.0 {
            parts.push(format!("送{}股", decimal(d.shares_dividend, 2)));
        }
        if !invalid && d.shares_into > 0.0 {
            parts.push(format!("转{}股", decimal(d.shares_into, 2)));
        }
        if !invalid && d.money > 0.0 {
            parts.push(format!("派{}元", decimal(d.money, 2)));
        }
        let info = if invalid {
            "-".to_string()
        } else if parts.is_empty() {
            "不分配".to_string()
        } else {
            format!("10股{}", parts.join("、"))
        };
        let date_dividend = if d.date_dividend.len() < 3 { "-" } else { &d.date_dividend };
        let date_record = if d.date_record.len() < 3 { "-" } else { &d.date_record };

        writeln!(writer, "{}", table_row(&[(&d.date, 10), (&info, 28), (date_dividend, 10), (date_record, 10)]))?;
    }
    Ok(())
}

fn write_presses<W: Write>(writer: &mut W, presses: &[Press], styled: bool) -> io::Result<()> {
    writeln!(writer, "\n{}", heading("最新公告", styled))?;

    for p in presses {
        write_field(writer, "公告日期", &p.date)?;
        write_field(writer, "公告标题", &p.title)?;
        write_field(writer, "公告链接", &p.url)?;
        writeln!(writer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use unicode_width::UnicodeWidthStr;

    use super::*;

    struct BrokenWriter;

    impl Write for BrokenWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn table_rows_use_display_width_and_remain_single_line() {
        let row = table_row(&[("中文", 6), ("A\nB\u{202e}C", 8), ("123", 5)]);

        assert_eq!(UnicodeWidthStr::width(row.as_str()), 6 + 3 + 8 + 3 + 5);
        assert!(!row.chars().any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}' | '\u{202e}')));
    }

    #[test]
    fn invalid_decimals_are_not_rendered_as_nan_or_infinity() {
        assert_eq!(decimal(f64::NAN, 2), "-");
        assert_eq!(decimal(f64::INFINITY, 4), "-");
        assert!(decimal(f64::MAX, 4).contains('e'));
        assert!(format_number(&f64::MAX).contains('e'));
        assert_eq!(positive_decimal(0.0, 2), "-");
        assert_eq!(positive_decimal(-1.0, 2), "-");
    }

    #[test]
    fn info_writer_bounds_untrusted_text_and_propagates_output_errors() {
        let value = format!("safe\n{}", "\u{301}".repeat(10_000));
        let mut output = Vec::new();
        write_field(&mut output, "字段", &value).unwrap();
        assert_eq!(String::from_utf8(output).unwrap().lines().count(), 1);

        let error = write_field(&mut BrokenWriter, "字段", "value").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
