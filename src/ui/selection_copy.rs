//! Lets a right-click menu copy the text the user had selected.
//!
//! egui collapses a label selection the moment any pointer button is pressed on a label, and its
//! selection state is private, so by the time a context menu is open the selection is gone. This
//! plugin runs ahead of that: when a secondary press arrives it holds the press back for one
//! frame, injects a `Copy` event so egui's labels report their selected text, and intercepts that
//! text on its way out (so the clipboard is left alone). The press is replayed on the next frame.

use egui::OutputCommand;
use egui::{Context, Event, FullOutput, PointerButton, RawInput};

#[derive(Default)]
pub struct SelectionCopy {
    /// Secondary-button events held back for one frame, in arrival order.
    delayed: Vec<Event>,
    /// A `Copy` event was injected this frame, so its output must be intercepted.
    capturing: bool,
    /// The text selected at the last secondary press, if any.
    snapshot: Option<String>,
}

impl SelectionCopy {
    /// Register the plugin (a no-op if it already is).
    pub fn install(ctx: &Context) {
        ctx.add_plugin(SelectionCopy::default());
    }

    /// The text that was selected when the latest right-click happened.
    pub fn selected(ctx: &Context) -> Option<String> {
        ctx.with_plugin::<SelectionCopy, _>(|p| p.snapshot.clone())
            .flatten()
    }
}

fn is_secondary_button(event: &Event) -> bool {
    matches!(
        event,
        Event::PointerButton {
            button: PointerButton::Secondary,
            ..
        }
    )
}

fn is_secondary_press(event: &Event) -> bool {
    matches!(
        event,
        Event::PointerButton {
            button: PointerButton::Secondary,
            pressed: true,
            ..
        }
    )
}

impl egui::plugin::Plugin for SelectionCopy {
    fn debug_name(&self) -> &'static str {
        "vettr::SelectionCopy"
    }

    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        // Replay events held back last frame ahead of anything new, keeping their order.
        let replay = std::mem::take(&mut self.delayed);
        let had_replay = !replay.is_empty();

        let mut holding = false;
        let mut kept = Vec::with_capacity(input.events.len());
        for event in input.events.drain(..) {
            if is_secondary_press(&event) {
                holding = true;
            }
            // Once a press is held back, hold its release too so the pair stays ordered.
            if holding && is_secondary_button(&event) {
                self.delayed.push(event);
            } else {
                kept.push(event);
            }
        }

        let mut events = replay;
        events.extend(kept);
        if holding {
            self.snapshot = None;
            self.capturing = true;
            events.push(Event::Copy);
        }
        input.events = events;

        if holding || had_replay {
            ctx.request_repaint();
        }
    }

    fn output_hook(&mut self, _ctx: &Context, output: &mut FullOutput) {
        if !std::mem::take(&mut self.capturing) {
            return;
        }
        let commands = &mut output.platform_output.commands;
        let mut text = None;
        commands.retain(|command| match command {
            OutputCommand::CopyText(copied) => {
                text = Some(copied.clone());
                false
            }
            _ => true,
        });
        self.snapshot = text.filter(|t| !t.is_empty());
    }
}
