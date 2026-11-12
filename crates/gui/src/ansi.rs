use gpui::{Rgba, rgb};

type Segment = (String, Option<Rgba>);

pub fn segments(text: &str, default: Option<Rgba>) -> Vec<Segment> {
    if text.is_empty() {
        return vec![(String::new(), default)];
    }
    let mut result = Vec::new();
    let mut plain = String::new();
    let mut color = default;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            if !ch.is_control() {
                plain.push(ch);
            } else if matches!(ch, '\n' | '\r' | '\t') {
                plain.push(' ');
            }
            continue;
        }
        if !plain.is_empty() {
            result.push((std::mem::take(&mut plain), color));
        }
        match chars.next() {
            Some('[') => {
                let mut params = String::new();
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        if ch == 'm' {
                            let values: Option<Vec<u16>> = params
                                .split(';')
                                .map(|s| {
                                    if s.is_empty() {
                                        Some(0)
                                    } else {
                                        s.parse().ok()
                                    }
                                })
                                .collect();
                            if let Some(values) = values {
                                sgr(&values, &mut color, default);
                            }
                        }
                        break;
                    }
                    params.push(ch);
                }
            }
            Some(']' | 'P' | '_' | '^') => {
                let mut escape = false;
                for ch in chars.by_ref() {
                    if ch == '\x07' || (escape && ch == '\\') {
                        break;
                    }
                    escape = ch == '\x1b';
                }
            }
            _ => {}
        }
    }
    if !plain.is_empty() {
        result.push((plain, color));
    }
    result
}

fn sgr(values: &[u16], color: &mut Option<Rgba>, default: Option<Rgba>) {
    let mut i = 0;
    while i < values.len() {
        match values[i] {
            0 | 39 => *color = default,
            n @ 30..=37 => *color = Some(indexed(n - 30)),
            n @ 90..=97 => *color = Some(indexed(n - 90 + 8)),
            38 | 48 | 58 => {
                let foreground = values[i] == 38;
                match values.get(i + 1) {
                    Some(5) if values.get(i + 2).is_some_and(|n| *n <= 255) => {
                        if foreground {
                            *color = Some(indexed(values[i + 2]));
                        }
                        i += 2;
                    }
                    Some(2)
                        if values
                            .get(i + 2..i + 5)
                            .is_some_and(|rgb| rgb.iter().all(|n| *n <= 255)) =>
                    {
                        if foreground {
                            *color = Some(rgb((u32::from(values[i + 2]) << 16)
                                | (u32::from(values[i + 3]) << 8)
                                | u32::from(values[i + 4])));
                        }
                        i += 4;
                    }
                    _ => break,
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn indexed(n: u16) -> Rgba {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    let [r, g, b] = match n {
        0..=15 => ANSI[n as usize],
        16..=231 => {
            let n = n - 16;
            let level = |v: u16| if v == 0 { 0 } else { 55 + 40 * v as u8 };
            [level(n / 36), level(n / 6 % 6), level(n % 6)]
        }
        _ => [8 + 10 * (n - 232) as u8; 3],
    };
    rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
}

#[cfg(test)]
#[path = "../tests/unit/ansi.rs"]
mod tests;
