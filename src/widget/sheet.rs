//! Sheet pane: tabs of severity-colored tables served by dash-core `GET /v1/sheets`.
//!
//! Row ordering and the contract live in `data::sheet`; this module only fetches,
//! keeps the last answer and draws it.

use data::sheet::{Config, Severity, Sheets, TableView, endpoint, selected_index, tab_views};
use iced::{
    Alignment, Color, Element, Length, Subscription, Theme,
    widget::{button, column, container, row, scrollable, text},
};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Shorter than the poll interval so a hung dash-core never stacks up requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone)]
pub enum Message {
    Fetched(Result<Sheets, String>),
    Select(String),
}

pub struct SheetPane {
    pub config: Config,
    tables: Vec<TableView>,
    generated_at: Option<String>,
    /// Last fetch failure; the previous tables stay visible under the banner.
    error: Option<String>,
}

impl SheetPane {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            tables: Vec::new(),
            generated_at: None,
            error: None,
        }
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Fetched(Ok(sheets)) => {
                self.tables = tab_views(&sheets);
                self.generated_at = Some(sheets.generated_at);
                self.error = None;
            }
            Message::Fetched(Err(error)) => {
                log::warn!("sheet: dash-core fetch failed: {error}");
                self.error = Some(error);
            }
            Message::Select(id) => self.config.selected = Some(id),
        }
    }

    pub fn subscription(
        &self,
        id: uuid::Uuid,
        window: iced::window::Id,
        pane: iced::widget::pane_grid::Pane,
    ) -> Subscription<Message> {
        // The URL is part of the identity: changing DASH_CORE_URL restarts the poll.
        Subscription::run_with((id, window, pane, endpoint()), |context| {
            let url = context.3.clone();
            iced::futures::stream::unfold(None, move |state: Option<reqwest::Client>| {
                let url = url.clone();
                async move {
                    let client = match state {
                        Some(client) => client,
                        None => match reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build() {
                            Ok(client) => client,
                            Err(error) => {
                                tokio::time::sleep(POLL_INTERVAL).await;
                                return Some((
                                    Message::Fetched(Err(format!("client HTTP: {error}"))),
                                    None,
                                ));
                            }
                        },
                    };
                    let result = fetch(&client, &url).await;
                    let message = Message::Fetched(result);
                    // Wait after the answer so the first poll fires immediately.
                    tokio::time::sleep(POLL_INTERVAL).await;
                    Some((message, Some(client)))
                }
            })
        })
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut body = column![].spacing(8).padding(8);

        if let Some(error) = &self.error {
            body = body.push(banner(data::sheet::unreachable_banner(error)));
        }

        let Some(index) = selected_index(&self.tables, self.config.selected.as_deref()) else {
            if self.error.is_none() {
                body = body.push(text("In attesa di dash-core…").size(13));
            }
            return body.into();
        };
        let table = &self.tables[index];

        let mut tabs = row![].spacing(6).align_y(Alignment::Center);
        for (i, tab) in self.tables.iter().enumerate() {
            tabs = tabs.push(tab_button(tab, i == index));
        }
        body = body.push(tabs);
        body = body.push(table_view(table));
        if let Some(generated_at) = &self.generated_at {
            let suffix = if self.error.is_some() {
                " (dati non aggiornati)"
            } else {
                ""
            };
            body = body.push(
                text(format!("generato: {generated_at}{suffix}"))
                    .size(10)
                    .style(crate::style::secondary_text),
            );
        }
        body.into()
    }
}

fn tab_button(table: &TableView, active: bool) -> Element<'_, Message> {
    let crit = table.crit_rows();
    let mut label = row![text(table.title.as_str()).size(13)]
        .spacing(6)
        .align_y(Alignment::Center);
    if crit > 0 {
        label = label.push(text(format!("{crit}")).size(10).style(|theme: &Theme| {
            iced::widget::text::Style {
                color: Some(severity_color(theme, Severity::Crit)),
            }
        }));
    }
    button(label)
        .padding([3, 10])
        .style(move |theme: &Theme, status| {
            if active {
                iced::widget::button::primary(theme, status)
            } else {
                iced::widget::button::secondary(theme, status)
            }
        })
        .on_press(Message::Select(table.id.clone()))
        .into()
}

fn banner(message: String) -> Element<'static, Message> {
    container(text(message).size(13))
        .padding([6, 10])
        .width(Length::Fill)
        .style(|theme: &Theme| {
            let color = severity_color(theme, Severity::Crit);
            container::Style {
                text_color: Some(color),
                background: Some(color.scale_alpha(0.16).into()),
                border: iced::Border {
                    color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

fn table_view(table: &TableView) -> Element<'_, Message> {
    let mut header = row![].spacing(2);
    for name in &table.columns {
        header = header.push(
            container(
                text(name.as_str())
                    .size(11)
                    .style(crate::style::secondary_text),
            )
            .padding([2, 6])
            .width(Length::FillPortion(1)),
        );
    }

    let mut rows = column![].spacing(2);
    for r in &table.rows {
        let mut line = row![].spacing(2);
        for cell in &r.cells {
            line = line.push(cell_view(&cell.text, cell.severity));
        }
        rows = rows.push(line);
    }
    if table.rows.is_empty() {
        rows = rows.push(text("nessuna riga").size(12));
    }

    column![header, scrollable(rows).height(Length::Fill)]
        .spacing(2)
        .into()
}

fn cell_view(value: &str, severity: Severity) -> Element<'_, Message> {
    container(text(value).size(12).font(crate::style::AZERET_MONO))
        .padding([3, 6])
        .width(Length::FillPortion(1))
        .style(move |theme: &Theme| {
            let color = severity_color(theme, severity);
            container::Style {
                text_color: Some(color),
                background: Some(color.scale_alpha(0.18).into()),
                border: iced::Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}

/// ok green, warn yellow, crit red come from the theme palette; stale and missing
/// have no palette slot, so they use fixed grey and purple.
pub fn severity_color(theme: &Theme, severity: Severity) -> Color {
    let palette = theme.palette();
    match severity {
        Severity::Ok => palette.success.base.color,
        Severity::Warn => palette.warning.base.color,
        Severity::Crit => palette.danger.base.color,
        Severity::Stale => Color::from_rgb8(0x8A, 0x8F, 0x98),
        Severity::Missing => Color::from_rgb8(0xA7, 0x6B, 0xE8),
    }
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Sheets, String> {
    let started = std::time::Instant::now();
    let response = client.get(url).send().await.map_err(|e| error_chain(&e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let bytes = response.bytes().await.map_err(|e| error_chain(&e))?;
    let sheets: Sheets =
        serde_json::from_slice(&bytes).map_err(|e| format!("risposta non valida: {e}"))?;
    log::debug!(
        "sheet: {} sheets in {} ms from {url}",
        sheets.sheets.len(),
        started.elapsed().as_millis()
    );
    Ok(sheets)
}

/// reqwest hides the cause ("connection refused") in the source chain.
fn error_chain(error: &reqwest::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        parts.push(cause.to_string());
        source = cause.source();
    }
    parts.join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severities_have_distinct_colors() {
        let theme = Theme::Dark;
        let all = [
            Severity::Ok,
            Severity::Warn,
            Severity::Crit,
            Severity::Stale,
            Severity::Missing,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(
                    severity_color(&theme, *a),
                    severity_color(&theme, *b),
                    "{a:?} vs {b:?}"
                );
            }
        }
    }

    const FIXTURE: &str = include_str!("../../fixtures/sheets.json");

    fn loaded() -> SheetPane {
        let sheets: Sheets = serde_json::from_str(FIXTURE).expect("fixture matches contract");
        let mut pane = SheetPane::new(Config::default());
        pane.update(Message::Fetched(Ok(sheets)));
        pane
    }

    #[test]
    fn ui_shows_tabs_crit_cell_and_no_banner_when_ok() {
        let pane = loaded();
        let mut ui = iced_test::simulator(pane.view());
        for title in ["Infra", "Alerts", "Rektslug"] {
            assert!(ui.find(title).is_ok(), "tab {title} not found");
        }
        // The first row is the crit one; its crit cell text must be on screen.
        let crit =
            data::sheet::tab_views(&serde_json::from_str::<Sheets>(FIXTURE).expect("fixture"))[0]
                .rows[0]
                .cells
                .iter()
                .find(|c| c.severity == Severity::Crit)
                .expect("crit cell")
                .text
                .clone();
        assert!(
            ui.find(crit.as_str()).is_ok(),
            "crit cell {crit:?} not found"
        );
        assert!(ui.find("dash-core non raggiungibile: refused").is_err());
    }

    #[test]
    fn ui_shows_banner_with_error_and_keeps_last_data() {
        let mut pane = loaded();
        pane.update(Message::Fetched(Err("connection refused".into())));
        let mut ui = iced_test::simulator(pane.view());
        assert!(
            ui.find("dash-core non raggiungibile: connection refused")
                .is_ok()
        );
        assert!(ui.find("Infra").is_ok(), "last data stays visible");
    }

    #[test]
    fn ui_shows_banner_before_any_data() {
        let mut pane = SheetPane::new(Config::default());
        pane.update(Message::Fetched(Err("HTTP 503 Service Unavailable".into())));
        let mut ui = iced_test::simulator(pane.view());
        assert!(
            ui.find("dash-core non raggiungibile: HTTP 503 Service Unavailable")
                .is_ok()
        );
    }

    #[test]
    fn clicking_a_tab_selects_it() {
        let pane = loaded();
        let mut ui = iced_test::simulator(pane.view());
        let _ = ui.click("Alerts");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Select(id) if id == "alerts")),
            "{messages:?}"
        );
    }

    /// Real e2e: reads the live dash-core at `DASH_CORE_URL` (default 127.0.0.1:8790) and
    /// checks that every sheet with at least one non-missing row shows a real cell on screen.
    /// Run with `cargo test -p flowsurface --bin flowsurface e2e_ -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a running dash-core; set DASH_CORE_URL"]
    fn e2e_reads_a_real_cell_for_every_reachable_sheet() {
        let url = data::sheet::endpoint();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let client = reqwest::Client::new();
        let sheets = runtime
            .block_on(fetch(&client, &url))
            .unwrap_or_else(|e| panic!("dash-core at {url}: {e}"));
        assert!(!sheets.sheets.is_empty(), "dash-core returned no sheets");

        let mut pane = SheetPane::new(Config::default());
        pane.update(Message::Fetched(Ok(sheets.clone())));
        let mut reachable = 0;
        for sheet in &sheets.sheets {
            let live = sheet
                .rows
                .iter()
                .find(|r| r.cells.iter().all(|c| c.severity != Severity::Missing));
            pane.update(Message::Select(sheet.id.clone()));
            let mut ui = iced_test::simulator(pane.view());
            match live {
                Some(row) => {
                    reachable += 1;
                    let text = &row.cells[0].value;
                    assert!(
                        ui.find(text.as_str()).is_ok(),
                        "sheet {}: real cell {text:?} not on screen",
                        sheet.id
                    );
                    println!("sheet {} reachable: cell {text:?}", sheet.id);
                }
                None => println!("sheet {} missing: no live row", sheet.id),
            }
        }
        assert!(reachable > 0, "no reachable sheet at {url}");
    }
}
