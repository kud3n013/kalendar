//! Tags: gathered from tasks, frames and the planner, and renamed, merged
//! or removed everywhere at once. Tags match whatever their case.

use super::*;

/// A tag as the settings show it: its spelling and how many tasks have it.
#[derive(Clone, PartialEq, Debug)]
pub struct TagUse {
    pub name: String,
    pub tasks: usize,
    pub frames: usize,
}

impl Data {
    /// Every tag in use, once each whatever its case, sorted by name.
    pub fn all_tags(&self) -> Vec<TagUse> {
        let mut out: Vec<TagUse> = Vec::new();
        let mut add = |name: &str, task: bool, frame: bool| {
            if name.trim().is_empty() {
                return;
            }
            let i = match out.iter().position(|u| u.name.eq_ignore_ascii_case(name)) {
                Some(i) => i,
                None => {
                    out.push(TagUse { name: name.to_string(), tasks: 0, frames: 0 });
                    out.len() - 1
                }
            };
            out[i].tasks += usize::from(task);
            out[i].frames += usize::from(frame);
        };
        for t in &self.tasks {
            for g in dedup(&t.tags) {
                add(g, true, false);
            }
        }
        for f in &self.frames {
            for g in dedup(&f.filter.tags) {
                add(g, false, true);
            }
        }
        for g in &self.prefs.frame_first_tags {
            add(g, false, false);
        }
        out.sort_by_key(|u| u.name.to_lowercase());
        out
    }

    /// Gives every tag in `from` the name `to`; several at once merges them.
    /// Returns how many tasks changed.
    pub fn rename_tags(&mut self, from: &[String], to: &str) -> usize {
        let to = to.trim().trim_start_matches('#').trim();
        if to.is_empty() || to.contains(char::is_whitespace) {
            return 0;
        }
        self.retag(from, Some(to))
    }

    /// Takes the tags in `names` off everything. Returns how many tasks changed.
    pub fn remove_tags(&mut self, names: &[String]) -> usize {
        self.retag(names, None)
    }

    fn retag(&mut self, from: &[String], to: Option<&str>) -> usize {
        let hit = |g: &String| from.iter().any(|f| f.eq_ignore_ascii_case(g));
        let fix = |tags: &mut Vec<String>| -> bool {
            if !tags.iter().any(hit) {
                return false;
            }
            let before = tags.clone();
            let mut out: Vec<String> = Vec::new();
            for g in tags.iter() {
                let g = if hit(g) {
                    match to {
                        Some(to) => to.to_string(),
                        None => continue,
                    }
                } else {
                    g.clone()
                };
                if !out.iter().any(|o| o.eq_ignore_ascii_case(&g)) {
                    out.push(g);
                }
            }
            *tags = out;
            *tags != before
        };
        let mut n = 0;
        for t in &mut self.tasks {
            n += usize::from(fix(&mut t.tags));
        }
        for f in &mut self.frames {
            fix(&mut f.filter.tags);
        }
        fix(&mut self.prefs.frame_first_tags);
        n
    }
}

/// Each tag once, whatever its case.
fn dedup(tags: &[String]) -> impl Iterator<Item = &String> {
    tags.iter().enumerate().filter(move |(i, g)| !tags[..*i].iter().any(|o| o.eq_ignore_ascii_case(g))).map(|(_, g)| g)
}
