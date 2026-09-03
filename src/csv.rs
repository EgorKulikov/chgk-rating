//! Row types and CSV builders for the site's roster and results importers.
//! Formats follow the rating.chgk.info admin documentation; see
//! `docs/har_notes.md` §4–5 for what the site actually does with them.

use std::collections::BTreeMap;

/// One player on one team, both keyed by rating.chgk.info ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRow {
    pub team_id: i64,
    pub team_name: String,
    pub town: String,
    pub player_id: i64,
    pub surname: String,
    pub name: String,
    pub patronymic: String,
    /// `К` (captain), `Б` (base squad), `Л` (legionnaire), or `None` when
    /// not tracked. Marking a player `Б` claims them as a permanent member
    /// of the team — leave it empty unless that is known to be true.
    pub flag: Option<char>,
}

/// `idteam,команда,город,признак (К|Б|Л),idplayer,Ф,И,О` — one row per
/// (team, player), comma-separated, UTF-8, no header.
pub fn build_rosters_csv(rows: &[RosterRow]) -> String {
    let mut s = String::new();
    for r in rows {
        s.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            r.team_id,
            escape(&r.team_name),
            escape(&r.town),
            r.flag.map(|c| c.to_string()).unwrap_or_default(),
            r.player_id,
            escape(&r.surname),
            escape(&r.name),
            escape(&r.patronymic),
        ));
    }
    s
}

/// A team's mark for one question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Correct,
    Wrong,
    /// Contested ("спорный"): the cell carries the disputed answer text
    /// from [`ResultsRoundRow::controversials`], or `?` when there is none.
    Contested,
    /// No mark recorded — an empty cell, which the site treats as wrong.
    Blank,
}

/// One team's marks for one round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsRoundRow {
    pub team_id: i64,
    pub team_name: String,
    pub town: String,
    /// 1-based round number.
    pub round: u32,
    /// One entry per question of the round, in order.
    pub marks: Vec<Mark>,
    /// For [`Mark::Contested`] cells: index into `marks` → disputed answer.
    pub controversials: BTreeMap<usize, String>,
}

/// `idteam,команда,город,номер тура,q1,q2,…` — one row per (team, round).
/// Cells are `1`, `0`, the contested answer text, or blank.
pub fn build_results_csv(rows: &[ResultsRoundRow]) -> String {
    let mut s = String::new();
    for r in rows {
        s.push_str(&format!(
            "{},{},{},{}",
            r.team_id,
            escape(&r.team_name),
            escape(&r.town),
            r.round,
        ));
        for (i, m) in r.marks.iter().enumerate() {
            s.push(',');
            match m {
                Mark::Correct => s.push('1'),
                Mark::Wrong => s.push('0'),
                Mark::Contested => {
                    let text = r.controversials.get(&i).map(String::as_str).unwrap_or("?");
                    s.push_str(&escape(text));
                }
                Mark::Blank => {}
            }
        }
        s.push('\n');
    }
    s
}

/// RFC-4180 field escaping. The site's importer (PhpSpreadsheet) reads
/// doubled quotes correctly; quotes cause trouble only in a team name that
/// does not match the registered one — see
/// [`crate::site::sanitize_roster_rows`].
pub fn escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(team_id: i64, team_name: &str, player_id: i64, flag: Option<char>) -> RosterRow {
        RosterRow {
            team_id,
            team_name: team_name.into(),
            town: "Краков".into(),
            player_id,
            surname: "Малкин".into(),
            name: "Михаил".into(),
            patronymic: "Леонидович".into(),
            flag,
        }
    }

    #[test]
    fn escape_only_when_needed() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape("has, comma"), "\"has, comma\"");
        assert_eq!(escape("has \"quote\""), "\"has \"\"quote\"\"\"");
        assert_eq!(escape("line\nfeed"), "\"line\nfeed\"");
    }

    #[test]
    fn rosters_csv_shape() {
        let rows = vec![
            row(86732, "4:20", 19541, None),
            row(107740, "\"Ладно, погнали втроём сегодня\"", 29516, Some('К')),
        ];
        let csv = build_rosters_csv(&rows);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "86732,4:20,Краков,,19541,Малкин,Михаил,Леонидович");
        assert_eq!(
            lines[1],
            "107740,\"\"\"Ладно, погнали втроём сегодня\"\"\",Краков,К,29516,Малкин,Михаил,Леонидович"
        );
    }

    #[test]
    fn results_csv_marks_and_contested() {
        let mut controversials = BTreeMap::new();
        controversials.insert(2, "спорный".to_string());
        let rows = vec![ResultsRoundRow {
            team_id: 42,
            team_name: "Прокрастинация".into(),
            town: "Berlin".into(),
            round: 1,
            marks: vec![Mark::Correct, Mark::Wrong, Mark::Contested, Mark::Blank, Mark::Contested],
            controversials,
        }];
        assert_eq!(
            build_results_csv(&rows),
            "42,Прокрастинация,Berlin,1,1,0,спорный,,?\n"
        );
    }
}
