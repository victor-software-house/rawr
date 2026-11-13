//! The calls that move the user around Rex: a session's windows and layers,
//! focusing a block, asking the Rex app to show a session, and moving or
//! floating a block.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::json;

use super::client::Client;

/// A session's windows, from `session.view`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct View {
    #[serde(default)]
    pub(crate) windows: Vec<Window>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Window {
    pub(crate) window_id: String,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) layers: Vec<Layer>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Layer {
    /// `tiled` for the layer that fills the window; anything else floats.
    pub(crate) kind: String,
    /// `[x, y, w, h]`, fractions of the window.
    pub(crate) bounds: [f64; 4],
    #[serde(default)]
    pub(crate) blocks: Vec<LayerBlock>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LayerBlock {
    pub(crate) block_id: String,
}

impl Layer {
    pub(crate) fn floats(&self) -> bool {
        self.kind != "tiled"
    }

    fn holds(&self, block_id: &str) -> bool {
        self.blocks.iter().any(|block| block.block_id == block_id)
    }
}

impl View {
    /// The window and layer that hold `block_id`, and the window's place in
    /// the tab order, from 1.
    pub(crate) fn find(&self, block_id: &str) -> Option<(usize, &Window, &Layer)> {
        self.windows.iter().enumerate().find_map(|(index, window)| {
            window
                .layers
                .iter()
                .find(|layer| layer.holds(block_id))
                .map(|layer| (index + 1, window, layer))
        })
    }
}

/// A client connected to the server, from `client.list`.
#[derive(Debug, Deserialize)]
struct Connected {
    client_id: String,
    #[serde(default)]
    info: Option<ClientInfo>,
    #[serde(default)]
    session_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ClientInfo {
    kind: String,
}

#[derive(Debug, Deserialize)]
struct ClientList {
    #[serde(default)]
    clients: Vec<Connected>,
}

impl Client {
    /// # Errors
    ///
    /// Fails when the server refuses the call.
    pub(crate) fn view(&mut self, session_id: &str) -> Result<View> {
        self.call("session.view", &json!({ "session_id": session_id }))
    }

    /// Focuses `block_id` and makes its window the session's active window.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the call.
    pub(crate) fn focus_block(&mut self, session_id: &str, block_id: &str) -> Result<()> {
        self.call::<_, IgnoredAny>(
            "session.focus_block",
            &json!({ "session_id": session_id, "block_id": block_id }),
        )?;
        Ok(())
    }

    /// Asks the Rex app attached to `near` (or the first app connected) to
    /// show `session_id`'s `window_id`, as its Go to Session action does.
    /// The app performs it only while its Remote Control setting is on.
    ///
    /// # Errors
    ///
    /// Fails when no app is connected or the app refuses.
    pub(crate) fn show(&mut self, near: &str, session_id: &str, window_id: &str) -> Result<()> {
        let list: ClientList = self.call("client.list", &json!({}))?;
        let apps: Vec<&Connected> = list
            .clients
            .iter()
            .filter(|client| client.info.as_ref().is_some_and(|info| info.kind == "app"))
            .collect();
        let app = apps
            .iter()
            .find(|client| client.session_ids.iter().any(|id| id == near))
            .or_else(|| apps.first())
            .context("no Rex app is connected to show the pane")?;
        self.call::<_, IgnoredAny>(
            "client.perform",
            &json!({
                "client_id": app.client_id,
                "action": "session.select",
                "args": { "session_id": session_id, "window_id": window_id },
                "origin": "api",
            }),
        )?;
        Ok(())
    }

    /// Moves `block_id` into a split beside `anchor`, in the same session.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the call.
    pub(crate) fn move_beside(
        &mut self,
        session_id: &str,
        block_id: &str,
        anchor: &str,
    ) -> Result<()> {
        self.call::<_, IgnoredAny>(
            "session.move_block",
            &json!({ "session_id": session_id, "block_id": block_id, "anchor_block_id": anchor }),
        )?;
        Ok(())
    }

    /// Lifts a tiled `block_id` into a floating layer at `bounds`.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the call.
    pub(crate) fn float(
        &mut self,
        session_id: &str,
        block_id: &str,
        bounds: [f64; 4],
    ) -> Result<()> {
        self.call::<_, IgnoredAny>(
            "session.float_block",
            &json!({ "session_id": session_id, "block_id": block_id, "bounds": bounds }),
        )?;
        Ok(())
    }

    /// Opens a floating layer at `bounds` in `window_id` that runs `command`,
    /// and focuses it.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the call.
    pub(crate) fn float_command(
        &mut self,
        session_id: &str,
        window_id: &str,
        bounds: [f64; 4],
        command: &[String],
    ) -> Result<()> {
        self.call::<_, IgnoredAny>(
            "session.new_layer",
            &json!({
                "session_id": session_id,
                "window_id": window_id,
                "bounds": bounds,
                "focus": true,
                "layout": { "block": {
                    "flavor": "com.superlogical.terminal.shell",
                    "options": { "command": command, "shell": "none" },
                } },
            }),
        )?;
        Ok(())
    }
}
