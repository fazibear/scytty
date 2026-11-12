use anyhow::Result;
use ghostty_vt::terminal::Point;

pub fn at_terminal(tab: &crate::Tab, point: Point) -> Result<Option<String>> {
    let reference = tab.terminal.grid_ref(point)?;
    let mut uri = vec![0; 8192];
    match reference.hyperlink_uri(&mut uri) {
        Ok(length) if length > 0 => {
            return Ok(std::str::from_utf8(&uri[..length])
                .ok()
                .filter(|url| safe_url(url))
                .map(str::to_owned));
        }
        // shortcut: links over 8 KiB are ignored; increase the bound if needed.
        Err(ghostty_vt::Error::OutOfSpace { .. }) => return Ok(None),
        Err(error) => return Err(error.into()),
        _ => {}
    }
    let Point::Viewport(coordinate) = point else {
        return Ok(None);
    };
    let mut row = String::new();
    let mut index = None;
    let mut previous = 0;
    for cell in tab
        .cells
        .iter()
        .filter(|cell| cell.y == coordinate.y as usize)
    {
        let start = row.len();
        if cell.x == usize::from(coordinate.x) {
            index = Some(if cell.text.is_empty() && !cell.narrow {
                previous
            } else {
                start
            });
        }
        if cell.text.is_empty() {
            if cell.narrow {
                row.push(' ');
            }
        } else {
            row.push_str(&cell.text);
        }
        previous = start;
    }
    Ok(index.and_then(|index| at(&row, index)).map(str::to_owned))
}

pub fn hovered_cells(tab: &crate::Tab, point: Point) -> Result<std::ops::Range<usize>> {
    let Point::Viewport(coordinate) = point else {
        return Ok(0..0);
    };
    let Some(url) = at_terminal(tab, point)? else {
        return Ok(0..0);
    };
    let Some(index) = tab
        .cells
        .iter()
        .position(|cell| cell.x == usize::from(coordinate.x) && cell.y == coordinate.y as usize)
    else {
        return Ok(0..0);
    };
    let matches = |index: usize| -> Result<bool> {
        let cell = &tab.cells[index];
        Ok(at_terminal(
            tab,
            Point::Viewport(ghostty_vt::terminal::PointCoordinate {
                x: cell.x as u16,
                y: cell.y as u32,
            }),
        )?
        .as_deref()
            == Some(url.as_str()))
    };
    let mut start = index;
    let mut end = index + 1;
    while start > 0 && matches(start - 1)? {
        start -= 1;
    }
    while end < tab.cells.len() && matches(end)? {
        end += 1;
    }
    Ok(start..end)
}

pub fn safe_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
        && !rest.split(['/', '?', '#']).next().unwrap_or("").is_empty()
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && !url.contains('\\')
}

pub fn at(text: &str, index: usize) -> Option<&str> {
    // shortcut: plain URLs are detected within a visible row; use OSC 8 for wrapped links.
    for (start, _) in text.match_indices("http") {
        let candidate = text[start..]
            .split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .next()?;
        let mut url = candidate.trim_end_matches(['.', ',', ';', '!', ':']);
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            while url.ends_with(close) && url.matches(close).count() > url.matches(open).count() {
                url = &url[..url.len() - close.len_utf8()];
            }
        }
        if index >= start && index < start + url.len() && safe_url(url) {
            return Some(url);
        }
    }
    None
}

#[cfg(test)]
#[path = "../tests/unit/links.rs"]
mod tests;
