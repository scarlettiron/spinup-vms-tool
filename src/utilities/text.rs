//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Text helpers: "did you mean" suggestions and table layout.

//How many single-letter edits turn `a` into `b` (the Levenshtein distance).
//Letter case is ignored.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();

    for (i, letter_a) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, letter_b) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(letter_a != letter_b);
            let insertion = current[j] + 1;
            let deletion = previous[j + 1] + 1;
            current.push(substitution.min(insertion).min(deletion));
        }
        previous = current;
    }
    previous[b.len()]
}

//The closest candidate to `word`, if one is close enough to be a likely typo.
pub fn closest<'a>(word: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (word.len() / 3).max(2);
    candidates
        .into_iter()
        .map(|candidate| (edit_distance(word, candidate), candidate))
        .filter(|(distance, _)| *distance <= limit)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

//Lays rows out in aligned columns, two spaces apart. The first row is the
//header. Widths count characters, not bytes, so "—" lines up.
pub fn table(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; columns];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }

    let mut output = String::new();
    for row in rows {
        let mut line = String::new();
        for (index, cell) in row.iter().enumerate() {
            let is_last = index + 1 == row.len();
            if is_last {
                line.push_str(cell);
            } else {
                let padding = widths[index] - cell.chars().count();
                line.push_str(cell);
                line.push_str(&" ".repeat(padding + 2));
            }
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_counts_changes() {
        assert_eq!(edit_distance("start", "strat"), 2);
        assert_eq!(edit_distance("CPUS", "cpus"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn closest_finds_typos_only() {
        assert_eq!(
            closest("memoryGb", ["MemoryGB", "DiskGB"]),
            Some("MemoryGB")
        );
        assert_eq!(closest("banana", ["MemoryGB", "DiskGB"]), None);
    }

    #[test]
    fn table_aligns_columns() {
        let rows = vec![
            vec!["NAME".to_string(), "STATUS".to_string()],
            vec!["acme".to_string(), "—".to_string()],
        ];
        assert_eq!(table(&rows), "NAME  STATUS\nacme  —\n");
    }
}
