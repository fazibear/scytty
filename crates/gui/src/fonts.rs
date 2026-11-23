use anyhow::{Context, Result};

pub fn resolve(family: &str) -> Result<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let query = if family == "monospace" {
        fontdb::Family::Monospace
    } else {
        fontdb::Family::Name(family)
    };
    let id = db
        .query(&fontdb::Query {
            families: &[query, fontdb::Family::Monospace],
            ..Default::default()
        })
        .with_context(|| {
            format!("neither font family {family} nor a system monospace font was found")
        })?;
    Ok(db.face(id).context("font disappeared")?.families[0]
        .0
        .clone())
}

#[cfg(test)]
#[path = "../tests/unit/fonts.rs"]
mod tests;
