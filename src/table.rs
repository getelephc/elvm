//! Column-aligned tables with a header row.
//!
//! Shared by `ls` and `ls-remote`, because the shape is not either one's
//! concern. Every cell is built before anything prints, so each column is
//! sized to the widest entry actually on screen — and a heading names what
//! the column holds, which is what lets these listings drop the sentences
//! they used to repeat on every row.

/// Prints `rows` under `headers`, aligned.
///
/// A column no row fills is dropped, heading and all: `ls-remote --all`
/// lists no collapsed series, so its `OLDER` column would otherwise be a
/// heading over nothing but spaces. Every line is right-trimmed, since
/// trailing spaces are invisible until someone selects or diffs the output.
///
/// A row shorter than `headers` is padded with empty cells rather than
/// panicking: a missing cell is missing information, not a crash.
pub fn print(headers: &[&str], rows: &[Vec<String>]) {
    let kept: Vec<usize> = (0..headers.len())
        .filter(|&index| rows.iter().any(|row| !cell(row, index).is_empty()))
        .collect();
    if kept.is_empty() {
        return;
    }

    // The heading counts toward its column's width: a wide heading over
    // narrow cells still has to line up with the columns after it.
    let width = |index: usize| {
        rows.iter()
            .map(|row| cell(row, index).chars().count())
            .chain(std::iter::once(headers[index].chars().count()))
            .max()
            .unwrap_or(0)
    };
    let widths: Vec<usize> = kept.iter().map(|&index| width(index)).collect();

    // `None` is the heading line, `Some(row)` a data line: one formatter,
    // so a heading can never drift out of step with the cells beneath it.
    for source in std::iter::once(None).chain(rows.iter().map(Some)) {
        let mut text = String::new();
        for (position, &index) in kept.iter().enumerate() {
            if position > 0 {
                text.push_str("  ");
            }
            let value = match source {
                None => headers[index],
                Some(row) => cell(row, index),
            };
            text.push_str(&pad(value, widths[position]));
        }
        println!("{}", text.trim_end());
    }
}

/// A row's cell, or `""` when the row is shorter than the header list.
fn cell(row: &[String], index: usize) -> &str {
    row.get(index).map(String::as_str).unwrap_or("")
}

/// Pads to `width` *display* columns. Counts chars, not bytes, so a version
/// string is not mismeasured by a multi-byte character.
fn pad(text: &str, width: usize) -> String {
    let mut padded = text.to_string();
    for _ in text.chars().count()..width {
        padded.push(' ');
    }
    padded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|cell| (*cell).to_string()).collect()
    }

    /// Mirrors `print`'s formatting without capturing stdout.
    fn render(headers: &[&str], rows: &[Vec<String>]) -> Vec<String> {
        let kept: Vec<usize> = (0..headers.len())
            .filter(|&i| rows.iter().any(|r| !cell(r, i).is_empty()))
            .collect();
        if kept.is_empty() {
            return Vec::new();
        }
        let widths: Vec<usize> = kept
            .iter()
            .map(|&i| {
                rows.iter()
                    .map(|r| cell(r, i).chars().count())
                    .chain(std::iter::once(headers[i].chars().count()))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let mut out = Vec::new();
        for source in std::iter::once(None).chain(rows.iter().map(Some)) {
            let mut text = String::new();
            for (position, &i) in kept.iter().enumerate() {
                if position > 0 {
                    text.push_str("  ");
                }
                let value = match source {
                    None => headers[i],
                    Some(r) => cell(r, i),
                };
                text.push_str(&pad(value, widths[position]));
            }
            out.push(text.trim_end().to_string());
        }
        out
    }

    #[test]
    fn lines_cells_up_under_their_headings() {
        let lines = render(
            &["VERSION", "OLDER"],
            &[row(&["0.19.14", "+14"]), row(&["0.25.2", "+2"])],
        );
        assert_eq!(lines, vec!["VERSION  OLDER", "0.19.14  +14", "0.25.2   +2"]);
    }

    #[test]
    fn a_heading_wider_than_its_cells_still_sets_the_column() {
        let lines = render(&["VERSION", "NOTE"], &[row(&["0.1.0", "x"])]);
        assert_eq!(lines, vec!["VERSION  NOTE", "0.1.0    x"]);
    }

    #[test]
    fn a_column_no_row_fills_is_dropped_with_its_heading() {
        // `ls-remote --all` collapses no series, so OLDER holds nothing —
        // printing the heading anyway would label an empty column.
        let lines = render(
            &["VERSION", "OLDER", "NOTE"],
            &[row(&["0.26.4", "", ""]), row(&["0.26.5", "", "latest"])],
        );
        assert_eq!(lines, vec!["VERSION  NOTE", "0.26.4", "0.26.5   latest"]);
    }

    #[test]
    fn no_line_ends_in_trailing_whitespace() {
        let lines = render(
            &["VERSION", "NOTE"],
            &[row(&["0.26.4", ""]), row(&["0.26.5", "latest"])],
        );
        for line in &lines {
            assert_eq!(line, line.trim_end(), "trailing space in {line:?}");
        }
    }

    #[test]
    fn width_is_counted_in_characters_not_bytes() {
        let lines = render(&["A", "B"], &[row(&["café", "x"]), row(&["ab", "y"])]);
        assert_eq!(lines, vec!["A     B", "café  x", "ab    y"]);
    }

    #[test]
    fn a_short_row_is_padded_rather_than_panicking() {
        let lines = render(&["A", "B"], &[row(&["only"]), row(&["x", "y"])]);
        assert_eq!(lines, vec!["A     B", "only", "x     y"]);
    }

    #[test]
    fn a_table_with_nothing_in_it_prints_nothing() {
        assert!(render(&["A", "B"], &[]).is_empty());
        assert!(render(&["A", "B"], &[row(&["", ""])]).is_empty());
    }
}
