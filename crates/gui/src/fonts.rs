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
            families: &[query],
            ..Default::default()
        })
        .with_context(|| format!("installed font family not found: {family}"))?;
    Ok(db.face(id).context("font disappeared")?.families[0]
        .0
        .clone())
}

#[cfg(test)]
mod tests {
    #[test]
    fn installed_fonts_only() {
        assert!(!super::resolve("monospace").unwrap().is_empty());
        assert!(super::resolve("nonexistent-scytty-font").is_err());
    }
}
