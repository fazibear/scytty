use anyhow::{Context, Result};
use eframe::egui::{FontData, FontDefinitions, FontFamily};

pub fn definitions(family: &str) -> Result<FontDefinitions> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let data = find(&db, family)?;
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(family.into(), data.into());
    for kind in [FontFamily::Monospace, FontFamily::Proportional] {
        fonts.families.insert(kind, vec![family.into()]);
    }
    Ok(fonts)
}

fn find(db: &fontdb::Database, family: &str) -> Result<FontData> {
    let family_query = if family == "monospace" {
        fontdb::Family::Monospace
    } else {
        fontdb::Family::Name(family)
    };
    let id = db
        .query(&fontdb::Query {
            families: &[family_query],
            ..Default::default()
        })
        .with_context(|| format!("installed font family not found: {family}"))?;
    db.with_face_data(id, |bytes, index| {
        let mut data = FontData::from_owned(bytes.to_vec());
        data.index = index;
        data
    })
    .with_context(|| format!("cannot read installed font: {family}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_fonts_only() -> Result<()> {
        let fonts = definitions("monospace")?;
        assert_eq!(fonts.font_data.len(), 1);
        assert_eq!(fonts.families[&FontFamily::Monospace], ["monospace"]);
        assert_eq!(fonts.families[&FontFamily::Proportional], ["monospace"]);
        assert!(find(&fontdb::Database::new(), "nonexistent-scytty-font").is_err());
        Ok(())
    }
}
