//! Sekme grupları: adı ve rengi olan, daraltılabilen sekme kümeleri.
//!
//! Kurallar:
//! - Bir grubun sekmeleri şeritte hep yan yanadır.
//! - Sabitlenmiş sekmeler gruba girmez.
//! - Sekmesi kalmayan grup silinir.

/// Grup renkleri: oturum dosyasındaki adı, Türkçe adı ve rengi.
pub const COLORS: [(&str, &str, u32); 9] = [
    ("grey", "Gri", 0x8b8f98),
    ("blue", "Mavi", 0x3b82f6),
    ("red", "Kırmızı", 0xef4444),
    ("yellow", "Sarı", 0xeab308),
    ("green", "Yeşil", 0x22c55e),
    ("pink", "Pembe", 0xec4899),
    ("purple", "Mor", 0xa855f7),
    ("cyan", "Turkuaz", 0x06b6d4),
    ("orange", "Turuncu", 0xf97316),
];

/// Grup adının en fazla uzunluğu (karakter).
pub const MAX_NAME: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: u64,
    pub name: String,
    /// `COLORS` içindeki sırası.
    pub color: usize,
    /// Daraltılmış: sekmeleri gizli, yalnızca başlığı görünür.
    pub collapsed: bool,
}

/// Oturum dosyasındaki renk adının sırası.
pub fn color_by_key(key: &str) -> Option<usize> {
    COLORS.iter().position(|(k, _, _)| *k == key)
}

/// Yeni grup için renk: henüz kullanılmayan ilk renk (hepsi kullanılıyorsa
/// sırayla). Gri, gruplar çoğalana kadar seçilmez.
pub fn next_color(groups: &[Group]) -> usize {
    let order = (1..COLORS.len()).chain(std::iter::once(0));
    order
        .clone()
        .find(|c| groups.iter().all(|g| g.color != *c))
        .unwrap_or_else(|| order.cycle().nth(groups.len()).unwrap_or(1))
}

/// Ad: satır sonu gibi kontrol karakterleri boşluk olur, uzunluk sınırlanır.
pub fn clean_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_NAME)
        .collect()
}

/// Sürüklenen sekme yeni yerine gelince hangi grupta olur: iki yanı aynı
/// gruptaysa (ya da ikisi de grupsuzsa) o; sınırdaysa ve kendi grubu
/// yanındaysa grubunda kalır; değilse grupsuz olur.
pub fn joined_group(prev: Option<u64>, own: Option<u64>, next: Option<u64>) -> Option<u64> {
    if prev == next {
        prev
    } else if own.is_some() && (own == prev || own == next) {
        own
    } else {
        None
    }
}

/// Kuralları uygular: sabitlenmiş sekmeler ve olmayan gruplar grupsuz
/// olur; bir grubun ikinci kez başlayan parçası gruptan çıkar; boş gruplar
/// silinir ve gruplar şeritteki sıralarına dizilir.
/// `tabs`: her sekme için (sabitlenmiş mi, grubu).
pub fn normalize(groups: &mut Vec<Group>, tabs: &mut [(bool, Option<u64>)]) {
    let mut seen: Vec<u64> = Vec::new();
    let mut prev = None;
    for (pinned, group) in tabs.iter_mut() {
        if *pinned {
            *group = None;
        }
        if let Some(id) = *group
            && (!groups.iter().any(|g| g.id == id) || (seen.contains(&id) && prev != Some(id)))
        {
            *group = None;
        }
        if let Some(id) = *group
            && !seen.contains(&id)
        {
            seen.push(id);
        }
        prev = *group;
    }
    groups.retain(|g| seen.contains(&g.id));
    groups.sort_by_key(|g| seen.iter().position(|id| *id == g.id));
}

/// Grubun şeritteki sekmeleri (ilk ve son sırası).
pub fn span(tabs: &[Option<u64>], id: u64) -> Option<(usize, usize)> {
    let first = tabs.iter().position(|g| *g == Some(id))?;
    let last = tabs.iter().rposition(|g| *g == Some(id))?;
    Some((first, last))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: u64, color: usize) -> Group {
        Group {
            id,
            name: String::new(),
            color,
            collapsed: false,
        }
    }

    #[test]
    fn colors_are_unused_first() {
        assert_eq!(next_color(&[]), 1);
        assert_eq!(next_color(&[group(1, 1), group(2, 2)]), 3);
        let all: Vec<Group> = (0..COLORS.len()).map(|c| group(c as u64, c)).collect();
        assert!(next_color(&all) < COLORS.len());
        assert_eq!(color_by_key("cyan"), Some(7));
        assert_eq!(color_by_key("nope"), None);
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("ai\nişler\t"), "ai işler ");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), MAX_NAME);
    }

    #[test]
    fn dragging_joins_and_leaves_groups() {
        // İki grup üyesinin arasına giren sekme gruba girer.
        assert_eq!(joined_group(Some(1), None, Some(1)), Some(1));
        // Grubun kenarına giden üye grubunda kalır.
        assert_eq!(joined_group(None, Some(1), Some(1)), Some(1));
        assert_eq!(joined_group(Some(1), Some(1), None), Some(1));
        // Gruptan dışarı çıkan sekme grupsuz olur.
        assert_eq!(joined_group(None, Some(1), None), None);
        // Grubun kenarına gelen yabancı sekme girmez.
        assert_eq!(joined_group(Some(1), None, Some(2)), None);
        assert_eq!(joined_group(Some(1), Some(3), Some(2)), None);
    }

    #[test]
    fn normalize_keeps_groups_together() {
        let mut groups = vec![group(2, 2), group(1, 1), group(9, 3)];
        let mut tabs = vec![
            (true, Some(1)),
            (false, Some(1)),
            (false, Some(1)),
            (false, None),
            (false, Some(1)),
            (false, Some(2)),
            (false, Some(7)),
        ];
        normalize(&mut groups, &mut tabs);
        assert_eq!(
            tabs,
            vec![
                (true, None),
                (false, Some(1)),
                (false, Some(1)),
                (false, None),
                (false, None),
                (false, Some(2)),
                (false, None),
            ]
        );
        // Boş grup (9) silindi, sıra şeritteki gibi.
        assert_eq!(groups.iter().map(|g| g.id).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn spans() {
        let tabs = [None, Some(1), Some(1), None];
        assert_eq!(span(&tabs, 1), Some((1, 2)));
        assert_eq!(span(&tabs, 2), None);
    }
}
