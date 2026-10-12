//! Bulk selection and explicit, authenticated Monarch edits.
use crate::app::App;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Merchant,
    Category,
}
impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Self::Merchant => "Merchant",
            Self::Category => "Category",
        }
    }
}
pub struct Dialog {
    pub field: Field,
    pub input: String,
    pub ids: Vec<String>,
    pub confirm: bool,
    pub suggestion: usize,
}
#[derive(Clone, Serialize)]
pub struct Job {
    pub ids: Vec<String>,
    pub field: Field,
    pub value: String,
}
#[derive(Deserialize)]
pub struct ResultRow {
    pub id: String,
    pub updated: bool,
    pub error: Option<String>,
}
#[derive(Deserialize)]
pub struct Outcome {
    pub results: Vec<ResultRow>,
}
pub fn apply(job: Job) -> Result<Outcome, String> {
    let url = std::env::var("NESTEGG_API_URL")
        .map_err(|_| "Set NESTEGG_API_URL to enable Monarch edits".to_string())?;
    let token = std::env::var("NESTEGG_API_TOKEN")
        .map_err(|_| "Set NESTEGG_API_TOKEN to enable Monarch edits".to_string())?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;
    // Do not automatically retry mutations after a timeout: the server may have applied them.
    let response = client
        .post(format!(
            "{}/v1/transaction-edits",
            url.trim_end_matches('/')
        ))
        .bearer_auth(token)
        .json(&job)
        .send()
        .map_err(|_| {
            "Connection interrupted; reload before retrying, as Monarch may have applied some edits"
                .to_string()
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let message = response.text().unwrap_or_default();
        return Err(format!(
            "Update rejected ({status}): {}",
            message.chars().take(200).collect::<String>()
        ));
    }
    let outcome: Outcome = response
        .json()
        .map_err(|_| "Invalid API response; reload before retrying".to_string())?;
    if outcome.results.len() != job.ids.len()
        || job
            .ids
            .iter()
            .any(|id| outcome.results.iter().filter(|r| &r.id == id).count() != 1)
    {
        return Err("Incomplete API response; reload before retrying".into());
    }
    Ok(outcome)
}
impl App {
    pub fn mark_transaction(&mut self) {
        if let Some(id) = self.selected_transaction().map(|t| t.id.clone()) {
            if !self.marked.remove(&id) {
                self.marked.insert(id);
            }
        }
    }
    pub fn begin_edit(&mut self, field: Field) {
        if self.edit_busy {
            self.status = "An update is already in progress".into();
            return;
        }
        if self.data.demo {
            self.status = "The demo household is not in Monarch to edit".into();
            return;
        }
        if self.data.anonymized {
            self.status = "Leave anonymized mode before editing Monarch".into();
            return;
        }
        if self.marked.is_empty() {
            self.status = "Mark transactions with t first".into();
            return;
        }
        if self.marked.len() > 500 {
            self.status = "Select at most 500 transactions per edit".into();
            return;
        }
        if std::env::var("NESTEGG_API_URL").is_err() || std::env::var("NESTEGG_API_TOKEN").is_err()
        {
            self.status = "Monarch editing API is not configured".into();
            return;
        }
        self.tx.detail = false;
        self.editor = Some(Dialog {
            field,
            input: String::new(),
            ids: self.marked.iter().cloned().collect(),
            confirm: false,
            suggestion: 0,
        });
    }
    pub fn edit_suggestions(&self) -> Vec<String> {
        let Some(dialog) = &self.editor else {
            return Vec::new();
        };
        if dialog.field != Field::Category || dialog.confirm {
            return Vec::new();
        }
        let needle = dialog.input.to_lowercase();
        let mut names: Vec<String> = self
            .data
            .categories
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| n.to_lowercase().contains(&needle))
            .collect();
        names.sort();
        names.dedup();
        names
    }
    pub fn handle_edit_key(&mut self, key: KeyEvent) {
        let suggestions = self.edit_suggestions();
        let Some(dialog) = self.editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.editor = None;
            }
            KeyCode::Enter if dialog.confirm => {
                self.edit_job = Some(Job {
                    ids: dialog.ids.clone(),
                    field: dialog.field,
                    value: dialog.input.trim().into(),
                });
                self.edit_busy = true;
                self.status = "Applying to Monarch…".into();
                self.editor = None;
            }
            KeyCode::Enter => {
                if dialog.field == Field::Category {
                    if let Some(name) = suggestions
                        .iter()
                        .find(|n| n.eq_ignore_ascii_case(dialog.input.trim()))
                        .or_else(|| suggestions.get(dialog.suggestion))
                    {
                        dialog.input = name.clone();
                    } else {
                        self.status = "Choose a Monarch category".into();
                        return;
                    }
                }
                let value = dialog.input.trim();
                if value.is_empty() || value.len() > 200 || value.chars().any(char::is_control) {
                    self.status = "Enter a name of 1–200 characters".into();
                    return;
                }
                dialog.input = value.to_string();
                dialog.confirm = true;
            }
            KeyCode::Up if !dialog.confirm => {
                dialog.suggestion = dialog.suggestion.saturating_sub(1)
            }
            KeyCode::Down if !dialog.confirm => {
                dialog.suggestion = (dialog.suggestion + 1).min(suggestions.len().saturating_sub(1))
            }
            KeyCode::Backspace if !dialog.confirm => {
                dialog.input.pop();
                dialog.suggestion = 0;
            }
            KeyCode::Char(c) if !dialog.confirm && !c.is_control() => {
                dialog.input.push(c);
                dialog.suggestion = 0;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Category, Data};
    use ratatui::crossterm::event::KeyModifiers;
    fn key(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    #[test]
    fn reviewing_an_edit_does_not_write_and_cancel_keeps_marks() {
        let mut app = App::new(Data::default());
        app.marked.insert("123".into());
        app.editor = Some(Dialog {
            field: Field::Merchant,
            input: "New merchant".into(),
            ids: vec!["123".into()],
            confirm: false,
            suggestion: 0,
        });
        key(&mut app, KeyCode::Enter);
        assert!(app.editor.as_ref().unwrap().confirm);
        assert!(app.edit_job.is_none());
        key(&mut app, KeyCode::Esc);
        assert!(app.editor.is_none());
        assert!(app.marked.contains("123"));
        assert!(app.edit_job.is_none());
    }
    #[test]
    fn category_choice_and_apply_capture_an_immutable_selection() {
        let mut data = Data::default();
        data.categories.push(Category {
            name: "Coffee Shops".into(),
            group_id: "food".into(),
        });
        let mut app = App::new(data);
        app.editor = Some(Dialog {
            field: Field::Category,
            input: "cof".into(),
            ids: vec!["123".into()],
            confirm: false,
            suggestion: 0,
        });
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.editor.as_ref().unwrap().input, "Coffee Shops");
        assert!(app.edit_job.is_none());
        key(&mut app, KeyCode::Enter);
        let job = app.edit_job.as_ref().unwrap();
        assert_eq!(job.ids, ["123"]);
        assert_eq!(job.value, "Coffee Shops");
        assert!(app.edit_busy);
        key(&mut app, KeyCode::Char('q'));
        assert!(!app.quit);
    }
    #[test]
    fn anonymized_mode_cannot_open_a_write_dialog() {
        let mut app = App::new(Data::default());
        app.data.anonymized = true;
        app.marked.insert("123".into());
        app.begin_edit(Field::Merchant);
        assert!(app.editor.is_none());
        assert!(app.edit_job.is_none());
    }
}
