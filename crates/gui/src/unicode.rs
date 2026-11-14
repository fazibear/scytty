use std::ops::Range;

// Native input ranges count UTF-16 units; never slice a surrogate pair in half.
pub(super) fn byte_range(text: &str, range: Range<usize>) -> Range<usize> {
    fn index(text: &str, offset: usize, round_up: bool) -> usize {
        let mut units = 0;
        for (i, ch) in text.char_indices() {
            if offset == units {
                return i;
            }
            units += ch.len_utf16();
            if offset < units {
                return i + if round_up { ch.len_utf8() } else { 0 };
            }
        }
        text.len()
    }
    let start = index(text, range.start, false);
    let end = if range.end > range.start {
        index(text, range.end, true)
    } else {
        start
    };
    start..end.max(start)
}

pub(super) fn utf16_range(text: &str, range: Range<usize>) -> Range<usize> {
    text[..range.start].encode_utf16().count()..text[..range.end].encode_utf16().count()
}

pub(super) fn replace_marked(
    marked: &mut String,
    range: Option<Range<usize>>,
    text: &str,
    selected: Option<Range<usize>>,
) -> Range<usize> {
    let range = range
        .map(|r| byte_range(marked, r))
        .unwrap_or(0..marked.len());
    let start = marked[..range.start].encode_utf16().count();
    marked.replace_range(range, text);
    let selected = selected
        .map(|r| utf16_range(text, byte_range(text, r)))
        .unwrap_or_else(|| {
            let end = text.encode_utf16().count();
            end..end
        });
    start + selected.start..start + selected.end
}

pub(super) fn cell_chars<'a>(
    reference: &ghostty_vt::screen::GridRef<'_>,
    buffer: &'a mut Vec<char>,
) -> anyhow::Result<&'a [char]> {
    if buffer.is_empty() {
        buffer.resize(32, '\0');
    }
    let length = match reference.graphemes(buffer) {
        Ok(length) => length,
        Err(ghostty_vt::Error::OutOfSpace { required }) => {
            buffer.resize(required, '\0');
            reference.graphemes(buffer)?
        }
        Err(error) => return Err(error.into()),
    };
    if length == 0 {
        buffer[0] = char::from_u32(reference.cell()?.codepoint()?)
            .filter(|ch| *ch != '\0')
            .unwrap_or(' ');
        Ok(&buffer[..1])
    } else {
        Ok(&buffer[..length])
    }
}

#[cfg(test)]
#[path = "../tests/unit/unicode.rs"]
mod tests;
