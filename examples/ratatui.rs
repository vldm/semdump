//! Note: This example should be built with `ratatui` feature flag enabled.

#[cfg(feature = "ratatui")]
mod impl_ {

    use ratatui::{
        crossterm::event,
        widgets::{Paragraph, Wrap},
    };
    use semdump::{DataPart, Ref, SemanticDump};

    fn test_data(size: u8) -> Vec<u8> {
        (0..=size).collect()
    }

    pub gfn create_dump() -> SemanticDump<'static> {
        let mut dump = SemanticDump::new(0);

        // Part can be created with builder pattern
        let mut part = DataPart::from_bytes(test_data(20));
        part.set_label("Small Part");

        dump.push_part(part);

        // Or as struct
        dump.push_part(DataPart {
            label: "Full range".to_string(),
            bytes: test_data(255).into(),
            refs: vec![
                Ref {
                    label: "Ref 1".to_string(),
                    range: 0x10..0x20,
                    extra_info: 0,
                },
                Ref {
                    label: "Ref 2".to_string(),
                    range: 0x30..0x50,
                    extra_info: 0,
                },
            ],
        });
        dump
    }

    pub fn ratatui_start(text: ratatui::text::Text) -> std::io::Result<()> {
        ratatui::run(move |terminal| {
            let text = text;
            loop {
                terminal.draw(|frame| {
                    let size = frame.area();
                    let paragraph = Paragraph::new(text.clone()).wrap(Wrap { trim: false });
                    frame.render_widget(paragraph, size);
                })?;
                if event::read()?.is_key_press() {
                    break Ok(());
                }
            }
        })
    }
}

#[cfg(feature = "ratatui")]
fn main() {
    // Simple pan with ratatui formatter used to render buffer in one span.
    let dump = impl_::create_dump();
    let mut formatter = semdump::RatatuiFormatter::new();
    dump.render(&mut formatter).unwrap();

    let result = formatter.into_text();
    impl_::ratatui_start(result).unwrap();
}

#[cfg(not(feature = "ratatui"))]
fn main() {
    println!("Please run with `ratatui` feature flag enabled.");
}
