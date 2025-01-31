use std::path::PathBuf;

use itertools::Itertools;

use crate::{
    error::Error,
    rename_tree::{Dir, File},
    rules::match_rule::{DirRule, FileRule, Selection, SortDirection},
};

use super::match_rule::{InsertionType, Position};

#[cfg(feature = "regex_match")]
use regex::Regex;

#[derive(Debug, Default)]
pub struct RuleEngine {
    global_index: usize,
    local_index: usize,
    dir_rules: Vec<DirRule>,
    file_rules: Vec<FileRule>,
}

impl RuleEngine {
    pub fn new(dir_rules: Vec<DirRule>, file_rules: Vec<FileRule>) -> Self {
        return Self {
            global_index: 0,
            local_index: 0,
            dir_rules,
            file_rules,
        };
    }

    pub fn process_dir(&mut self, mut dir: Dir) -> Result<Vec<File>, Error> {
        self.local_index = 0;

        let mut files = std::mem::take(&mut dir.contents);

        for rule in self.dir_rules.clone() {
            self.execute_dir_rule(&rule, &mut files);
        }

        for rule in &dir.dir_rules {
            self.execute_dir_rule(&rule, &mut files);
        }

        for f in &mut files {
            self.run_file(f)?;
        }

        return Ok(files);
    }

    pub fn process_file(&mut self, file: &mut File) -> Result<(), Error> {
        self.local_index = 0;

        return self.run_file(file);
    }

    fn run_file(&mut self, file: &mut File) -> Result<(), Error> {
        for rule in &self.file_rules {
            self.execute_file_rule(rule, &mut file.destination)?;
        }

        for rule in &file.rules {
            self.execute_file_rule(rule, &mut file.destination)?;
        }

        self.global_index += 1;
        self.local_index += 1;

        return Ok(());
    }

    fn execute_dir_rule(&mut self, rule: &DirRule, input: &mut Vec<File>) {
        match rule {
            DirRule::Sort(d) => Self::sort(*d, input),
            DirRule::Remove(rule) => {
                let filtered = input
                    .drain(0..)
                    .filter(|f| !rule.resolve(&f.destination.display().to_string()))
                    .collect_vec();

                let _ = std::mem::replace(input, filtered);
            }
            DirRule::IncludeOnly(rule) => {
                let filtered = input
                    .drain(0..)
                    .filter(|f| rule.resolve(&f.destination.display().to_string()))
                    .collect_vec();

                let _ = std::mem::replace(input, filtered);
            }
            DirRule::OffsetLocalIndex(i) => self.local_index = *i,
        }
    }

    fn sort(direction: SortDirection, input: &mut Vec<File>) {
        match direction {
            SortDirection::Ascending => input.sort_by(|a, b| a.destination.cmp(&b.destination)),
            SortDirection::Descending => input.sort_by(|a, b| b.destination.cmp(&a.destination)),
        }
    }

    fn execute_file_rule(&self, rule: &FileRule, input: &mut PathBuf) -> Result<bool, Error> {
        match rule {
            #[cfg(feature = "regex_match")]
            FileRule::RegexReplace(selection, find, replace) => {
                let new_f_name = match input
                    .file_name()
                    .map(|f_name| f_name.to_os_string().into_string())
                {
                    Some(Ok(f_name)) => Self::regex_replace(f_name, *selection, find, replace),
                    _ => return Err(Error::CannotIdentifyFileName),
                };

                input.set_file_name(new_f_name);
            }
            FileRule::Replace(selection, find, replace) => {
                let new_f_name = match input
                    .file_name()
                    .map(|f_name| f_name.to_os_string().into_string())
                {
                    Some(Ok(f_name)) => Self::replace(f_name, *selection, find, replace),
                    _ => return Err(Error::CannotIdentifyFileName),
                };

                input.set_file_name(new_f_name);
            }
            FileRule::Insert(pos, tp) => {
                let content = match tp {
                    InsertionType::LocalIndex => self.local_index.to_string(),
                    InsertionType::OverallIndex => self.global_index.to_string(),
                    InsertionType::Static(s) => s.clone(),
                };

                let mut old_f_name = input
                    .file_name()
                    .ok_or(Error::CannotIdentifyFileName)
                    .map(|f_name| {
                        f_name
                            .to_os_string()
                            .into_string()
                            .map_err(|_| Error::CannotIdentifyFileName)
                    })??;

                let new_f_name = match pos {
                    Position::Index(i) => {
                        if *i > old_f_name.len() {
                            return Err(Error::InsertIndexTooLarge);
                        }

                        old_f_name.insert_str(*i, &content);

                        old_f_name
                    }
                    Position::After(f) => {
                        if let Some(i) = old_f_name.find(f) {
                            if i + f.len() > old_f_name.len() {
                                old_f_name.push_str(&content);
                            } else {
                                old_f_name.insert_str(i + f.len(), &content);
                            }
                        }

                        old_f_name
                    }
                    Position::Before(f) => {
                        if let Some(i) = old_f_name.find(f) {
                            old_f_name.insert_str(i, &content);
                        }

                        old_f_name
                    }
                    Position::Start => {
                        let mut c = content;
                        c.push_str(&old_f_name);

                        c
                    }
                    Position::End => {
                        old_f_name.push_str(&content);

                        old_f_name
                    }
                };

                input.set_file_name(new_f_name);
            }
            FileRule::Set(s) => input.set_file_name(s),
            FileRule::SkipIf(rule) => {
                if rule.resolve(&input.display().to_string()) {
                    return Ok(false);
                }
            }
            FileRule::Left(m, inclusive) => {
                input.set_file_name(Self::left(Self::get_file_name(input)?, m, *inclusive));
            }
            FileRule::Right(m, inclusive) => {
                input.set_file_name(Self::right(Self::get_file_name(input)?, m, *inclusive));
            }
            #[cfg(feature = "regex_match")]
            FileRule::RegexLeft(reg, inclusive) => {
                input.set_file_name(Self::regex_left(
                    Self::get_file_name(input)?,
                    reg,
                    *inclusive,
                ));
            }
            #[cfg(feature = "regex_match")]
            FileRule::RegexRight(reg, inclusive) => {
                input.set_file_name(Self::regex_right(
                    Self::get_file_name(input)?,
                    reg,
                    *inclusive,
                ));
            }
        };

        return Ok(true);
    }

    #[cfg(feature = "regex_match")]
    fn regex_left(mut input: String, reg: &Regex, inclusive: bool) -> String {
        if let Some(m) = reg.find(&input) {
            if inclusive {
                input = input[..m.end()].to_string();
            } else {
                input = input[..m.start()].to_string();
            }
        }

        return input;
    }

    #[cfg(feature = "regex_match")]
    fn regex_right(mut input: String, reg: &Regex, inclusive: bool) -> String {
        if let Some(m) = reg.find(&input) {
            if inclusive {
                input = input[m.start()..].to_string();
            } else {
                input = input[m.end()..].to_string();
            }
        }

        return input;
    }

    fn left(mut input: String, match_str: &str, inclusive: bool) -> String {
        if let Some(mut slice) = input.find(match_str) {
            if inclusive {
                slice += match_str.len();
            }

            input = input[..slice].to_string()
        }

        return input;
    }

    fn right(mut input: String, match_str: &str, inclusive: bool) -> String {
        if let Some(mut slice) = input.find(match_str) {
            if !inclusive {
                slice += match_str.len();
            }

            input = input[slice..].to_string()
        }

        return input;
    }

    fn replace(input: String, selection: Selection, find: &String, replace: &String) -> String {
        return match selection {
            Selection::First => {
                // Could be better optimized

                if let Some(slice) = input.find(find) {
                    return [
                        &input[0..slice],
                        replace.as_str(),
                        &input[slice + find.len()..],
                    ]
                    .join("");
                } else {
                    return input;
                }
            }
            Selection::Last => {
                // Could be better optimized

                if let Some(slice) = input.rfind(find) {
                    return [
                        &input[0..slice],
                        replace.as_str(),
                        &input[slice + find.len()..],
                    ]
                    .join("");
                } else {
                    return input;
                }
            }
            Selection::All => input.replace(find, replace),
        };
    }

    #[cfg(feature = "regex_match")]
    fn regex_replace(input: String, selection: Selection, find: &Regex, replace: &str) -> String {
        return match selection {
            Selection::First => find.replace(&input, replace).to_string(),
            Selection::Last => {
                let i = find.find_iter(&input);

                if let Some(m) = i.last() {
                    format!("{}{}{}", &input[0..m.start()], replace, &input[m.end()..])
                } else {
                    input
                }
            }
            Selection::All => find.replace_all(&input, replace).to_string(),
        };
    }

    fn get_file_name(path: &PathBuf) -> Result<String, Error> {
        return path
            .file_name()
            .map(|f_name| {
                f_name
                    .to_os_string()
                    .into_string()
                    .map_err(|_| Error::CannotIdentifyFileName)
            })
            .ok_or(Error::CannotIdentifyFileName)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_first_1() {
        assert_eq!(
            RuleEngine::replace(
                "test message hello".to_string(),
                Selection::First,
                &"message".to_string(),
                &"yo".to_string()
            ),
            "test yo hello"
        );
    }

    #[test]
    fn test_replace_first_2() {
        assert_eq!(
            RuleEngine::replace(
                "test message message hello".to_string(),
                Selection::First,
                &"message".to_string(),
                &"yo".to_string()
            ),
            "test yo message hello"
        );
    }

    #[test]
    fn test_replace_last_1() {
        assert_eq!(
            RuleEngine::replace(
                "test message hello".to_string(),
                Selection::Last,
                &"message".to_string(),
                &"yo".to_string()
            ),
            "test yo hello"
        );
    }

    #[test]
    fn test_replace_last_2() {
        assert_eq!(
            RuleEngine::replace(
                "test message message hello".to_string(),
                Selection::Last,
                &"message".to_string(),
                &"yo".to_string()
            ),
            "test message yo hello"
        );
    }

    #[test]
    fn test_left_1() {
        assert_eq!(
            RuleEngine::left(
                "test message message hello".to_string(),
                &"message".to_string(),
                true
            ),
            "test message"
        );
    }

    #[test]
    fn test_left_2() {
        assert_eq!(
            RuleEngine::left(
                "test message message hello".to_string(),
                &"message".to_string(),
                false
            ),
            "test "
        );
    }

    #[test]
    fn test_right_1() {
        assert_eq!(
            RuleEngine::right(
                "test message message hello".to_string(),
                &"message".to_string(),
                true
            ),
            "message message hello"
        );
    }

    #[test]
    fn test_right_2() {
        assert_eq!(
            RuleEngine::right(
                "test message message hello".to_string(),
                &"message".to_string(),
                false
            ),
            " message hello"
        );
    }

    #[cfg(feature = "regex_match")]
    mod regex {
        use super::*;

        #[test]
        fn test_regex_replace_first() {
            let r = Regex::new("test").unwrap();
            let input = "test cow test".to_string();

            let output = RuleEngine::regex_replace(input, Selection::First, &r, "cow");

            assert_eq!(output, "cow cow test");
        }

        #[test]
        fn test_regex_replace_last() {
            let r = Regex::new("test").unwrap();
            let input = "test cow test".to_string();

            let output = RuleEngine::regex_replace(input, Selection::Last, &r, "cow");

            assert_eq!(output, "test cow cow");
        }

        #[test]
        fn test_regex_replace_all() {
            let r = Regex::new("test").unwrap();
            let input = "test cow test".to_string();

            let output = RuleEngine::regex_replace(input, Selection::All, &r, "cow");

            assert_eq!(output, "cow cow cow");
        }

        #[test]
        fn test_regex_left_1() {
            assert_eq!(
                RuleEngine::regex_left(
                    "test message message hello".to_string(),
                    &Regex::new("message").unwrap(),
                    true
                ),
                "test message"
            );
        }

        #[test]
        fn test_regex_left_2() {
            assert_eq!(
                RuleEngine::regex_left(
                    "test message message hello".to_string(),
                    &Regex::new("message").unwrap(),
                    false
                ),
                "test "
            );
        }

        #[test]
        fn test_regex_right_1() {
            assert_eq!(
                RuleEngine::regex_right(
                    "test message message hello".to_string(),
                    &Regex::new("message").unwrap(),
                    true
                ),
                "message message hello"
            );
        }

        #[test]
        fn test_regex_right_2() {
            assert_eq!(
                RuleEngine::regex_right(
                    "test message message hello".to_string(),
                    &Regex::new("message").unwrap(),
                    false
                ),
                " message hello"
            );
        }
    }
}
