//! 股票搜索命令

use std::io::{self, Write, stdout};

use anyhow::{Result, bail};

use crate::{format::fit_text, invest::Investment, provider::Provider};

/// 处理搜索命令
pub async fn handle_search<P: Provider>(provider: &P, query: &str, limit: usize) -> Result<()> {
    if limit == 0 {
        bail!("展示条数必须大于 0");
    }
    let results = provider.search(query).await?;
    if results.is_empty() {
        bail!("未找到匹配的证券");
    }

    let count = limit.min(results.len());
    let stdout = stdout();
    let mut writer = stdout.lock();
    write_results(&mut writer, &results[..count])?;
    writer.flush()?;
    Ok(())
}

fn write_results<W: Write>(writer: &mut W, results: &[Investment]) -> io::Result<()> {
    for invest in results {
        writeln!(writer, "{} │ {}", fit_text(&invest.symbol, 8), fit_text(&invest.name, 32))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
    fn search_writer_sanitizes_lines_and_propagates_output_errors() {
        let results =
            [Investment { symbol: "SH601318\nX".into(), name: "平安\r\n恶意".into(), ..Default::default() }];
        let mut output = Vec::new();
        write_results(&mut output, &results).unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(rendered.lines().count(), 1);

        let error = write_results(&mut BrokenWriter, &results).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
