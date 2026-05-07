use std::sync::{Arc, Mutex};

use gtk4 as gtk;
use gtk4::prelude::*;
use ironrdp::cliprdr::backend::{ClipboardMessage, CliprdrBackend};
use ironrdp::cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest,
    FileContentsResponse, FormatDataRequest, FormatDataResponse, LockDataId,
};
use ironrdp::cliprdr::{Client, CliprdrClient, CliprdrSvcMessages};
use ironrdp::core::{AsAny, IntoOwned as _};
use ironrdp::session::{ActiveStage, ActiveStageOutput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RdpClipboardCommand {
    LocalTextChanged(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostClipboardCommand {
    SetText(String),
}

#[derive(Debug, Default)]
struct TextClipboardState {
    local_text: Option<String>,
    pending_remote_format: Option<ClipboardFormatId>,
}

#[derive(Debug)]
pub(crate) struct RdpClipboardRuntime {
    pub(crate) command_rx: async_channel::Receiver<RdpClipboardCommand>,
    pub(crate) backend_rx: async_channel::Receiver<ClipboardMessage>,
    state: Arc<Mutex<TextClipboardState>>,
}

impl RdpClipboardRuntime {
    pub(crate) fn set_local_text(&self, text: String) {
        if let Ok(mut state) = self.state.lock() {
            state.local_text = Some(text);
        }
    }
}

#[derive(Debug)]
pub(crate) struct TextClipboardBackend {
    temp_dir: String,
    state: Arc<Mutex<TextClipboardState>>,
    backend_tx: async_channel::Sender<ClipboardMessage>,
    host_tx: async_channel::Sender<HostClipboardCommand>,
}

impl AsAny for TextClipboardBackend {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }
}

pub(crate) fn create_text_clipboard_bridge() -> (
    TextClipboardBackend,
    RdpClipboardRuntime,
    async_channel::Sender<RdpClipboardCommand>,
    async_channel::Receiver<HostClipboardCommand>,
) {
    let state = Arc::new(Mutex::new(TextClipboardState::default()));
    let (command_tx, command_rx) = async_channel::unbounded();
    let (backend_tx, backend_rx) = async_channel::unbounded();
    let (host_tx, host_rx) = async_channel::unbounded();

    let backend = TextClipboardBackend {
        temp_dir: std::env::temp_dir().to_string_lossy().into_owned(),
        state: state.clone(),
        backend_tx,
        host_tx,
    };
    let runtime = RdpClipboardRuntime {
        command_rx,
        backend_rx,
        state,
    };

    (backend, runtime, command_tx, host_rx)
}

pub(crate) fn text_clipboard_formats() -> Vec<ClipboardFormat> {
    vec![
        ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT),
        ClipboardFormat::new(ClipboardFormatId::CF_TEXT),
    ]
}

pub(crate) fn install_gtk_text_clipboard_bridge(
    command_tx: async_channel::Sender<RdpClipboardCommand>,
    host_rx: async_channel::Receiver<HostClipboardCommand>,
) {
    let Some(display) = gtk::gdk::Display::default() else {
        tracing::warn!("GTK display unavailable; clipboard bridge disabled");
        return;
    };
    let clipboard = display.clipboard();
    let remote_marker: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    {
        let command_tx = command_tx.clone();
        let remote_marker = remote_marker.clone();
        clipboard.connect_changed(move |clipboard| {
            let command_tx = command_tx.clone();
            let remote_marker = remote_marker.clone();
            clipboard.read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
                let Ok(Some(text)) = result else {
                    return;
                };
                let text = text.to_string();
                if remote_marker.borrow().as_deref() == Some(text.as_str()) {
                    *remote_marker.borrow_mut() = None;
                    return;
                }
                let _ = command_tx.try_send(RdpClipboardCommand::LocalTextChanged(text));
            });
        });
    }

    glib::MainContext::default().spawn_local(async move {
        while let Ok(command) = host_rx.recv().await {
            match command {
                HostClipboardCommand::SetText(text) => {
                    *remote_marker.borrow_mut() = Some(text.clone());
                    clipboard.set_text(&text);
                }
            }
        }
    });

    let command_tx = command_tx.clone();
    let clipboard_for_initial_read = display.clipboard();
    clipboard_for_initial_read.read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
        if let Ok(Some(text)) = result {
            let _ = command_tx.try_send(RdpClipboardCommand::LocalTextChanged(text.to_string()));
        }
    });
}

pub(crate) fn process_clipboard_command(
    active_stage: &mut ActiveStage,
    runtime: &RdpClipboardRuntime,
    command: RdpClipboardCommand,
) -> crate::error::WinbridgeResult<Vec<ActiveStageOutput>> {
    match command {
        RdpClipboardCommand::LocalTextChanged(text) => {
            runtime.set_local_text(text);
            process_clipboard_message(
                active_stage,
                ClipboardMessage::SendInitiateCopy(text_clipboard_formats()),
            )
        }
    }
}

pub(crate) fn process_clipboard_message(
    active_stage: &mut ActiveStage,
    message: ClipboardMessage,
) -> crate::error::WinbridgeResult<Vec<ActiveStageOutput>> {
    let messages = match message {
        ClipboardMessage::SendInitiateCopy(formats) => {
            with_cliprdr(active_stage, |cliprdr| cliprdr.initiate_copy(&formats))?
        }
        ClipboardMessage::SendFormatData(response) => {
            with_cliprdr(active_stage, |cliprdr| cliprdr.submit_format_data(response))?
        }
        ClipboardMessage::SendInitiatePaste(format) => {
            with_cliprdr(active_stage, |cliprdr| cliprdr.initiate_paste(format))?
        }
        ClipboardMessage::Error(err) => {
            tracing::warn!("clipboard backend error: {err}");
            return Ok(Vec::new());
        }
    };

    let frame = active_stage
        .process_svc_processor_messages(messages)
        .map_err(|err| {
            crate::error::RdpError::Protocol(format!("CLIPRDR 메시지 인코딩 실패: {err}"))
        })?;

    if frame.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![ActiveStageOutput::ResponseFrame(frame)])
    }
}

fn with_cliprdr(
    active_stage: &mut ActiveStage,
    f: impl FnOnce(&CliprdrClient) -> ironrdp::pdu::PduResult<CliprdrSvcMessages<Client>>,
) -> crate::error::WinbridgeResult<CliprdrSvcMessages<Client>> {
    let Some(cliprdr) = active_stage.get_svc_processor::<CliprdrClient>() else {
        tracing::debug!("CLIPRDR static channel is not available");
        return Ok(Vec::new().into());
    };

    f(cliprdr)
        .map_err(|err| crate::error::RdpError::Protocol(format!("CLIPRDR 처리 실패: {err}")).into())
}

impl CliprdrBackend for TextClipboardBackend {
    fn temporary_directory(&self) -> &str {
        &self.temp_dir
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::empty()
    }

    fn on_ready(&mut self) {}

    fn on_request_format_list(&mut self) {
        if self
            .state
            .lock()
            .ok()
            .and_then(|state| state.local_text.clone())
            .is_some()
        {
            let _ = self
                .backend_tx
                .try_send(ClipboardMessage::SendInitiateCopy(text_clipboard_formats()));
        }
    }

    fn on_process_negotiated_capabilities(
        &mut self,
        _capabilities: ClipboardGeneralCapabilityFlags,
    ) {
    }

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        let requested_format = select_text_format(available_formats);
        if let Ok(mut state) = self.state.lock() {
            state.pending_remote_format = requested_format;
        }

        if let Some(format) = requested_format {
            let _ = self
                .backend_tx
                .try_send(ClipboardMessage::SendInitiatePaste(format));
        }
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        let response = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.local_text.clone())
            .map(|text| format_data_response_for_text(request.format, &text))
            .unwrap_or_else(FormatDataResponse::new_error);

        let _ = self
            .backend_tx
            .try_send(ClipboardMessage::SendFormatData(response.into_owned()));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        if response.is_error() {
            return;
        }

        let requested_format = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.pending_remote_format);
        let Some(requested_format) = requested_format else {
            return;
        };

        let text = if requested_format == ClipboardFormatId::CF_UNICODETEXT {
            response.to_unicode_string()
        } else if requested_format == ClipboardFormatId::CF_TEXT {
            response.to_string()
        } else {
            return;
        };

        if let Ok(text) = text {
            if let Ok(mut state) = self.state.lock() {
                state.local_text = Some(text.clone());
            }
            let _ = self.host_tx.try_send(HostClipboardCommand::SetText(text));
        }
    }

    fn on_file_contents_request(&mut self, _request: FileContentsRequest) {}

    fn on_file_contents_response(&mut self, _response: FileContentsResponse<'_>) {}

    fn on_lock(&mut self, _data_id: LockDataId) {}

    fn on_unlock(&mut self, _data_id: LockDataId) {}
}

fn select_text_format(available_formats: &[ClipboardFormat]) -> Option<ClipboardFormatId> {
    if available_formats
        .iter()
        .any(|format| format.id() == ClipboardFormatId::CF_UNICODETEXT)
    {
        Some(ClipboardFormatId::CF_UNICODETEXT)
    } else if available_formats
        .iter()
        .any(|format| format.id() == ClipboardFormatId::CF_TEXT)
    {
        Some(ClipboardFormatId::CF_TEXT)
    } else {
        None
    }
}

fn format_data_response_for_text(
    format: ClipboardFormatId,
    text: &str,
) -> FormatDataResponse<'static> {
    if format == ClipboardFormatId::CF_UNICODETEXT {
        FormatDataResponse::new_unicode_string(text)
    } else if format == ClipboardFormatId::CF_TEXT {
        FormatDataResponse::new_string(text)
    } else {
        FormatDataResponse::new_error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ironrdp::cliprdr::backend::ClipboardMessage;
    use ironrdp::cliprdr::backend::CliprdrBackend;
    use ironrdp::cliprdr::pdu::{ClipboardFormatId, FormatDataRequest, FormatDataResponse};

    #[test]
    fn text_clipboard_formats_advertise_unicode_text_first() {
        let formats = text_clipboard_formats();

        assert_eq!(formats[0].id(), ClipboardFormatId::CF_UNICODETEXT);
        assert_eq!(formats[1].id(), ClipboardFormatId::CF_TEXT);
    }

    #[test]
    fn backend_sends_local_unicode_text_when_remote_requests_format_data() {
        let (mut backend, runtime, _command_tx, _host_rx) = create_text_clipboard_bridge();
        runtime.set_local_text("안녕".to_string());

        backend.on_format_data_request(FormatDataRequest {
            format: ClipboardFormatId::CF_UNICODETEXT,
        });

        let ClipboardMessage::SendFormatData(response) =
            runtime.backend_rx.try_recv().expect("format data response")
        else {
            panic!("expected format data response");
        };
        assert_eq!(response.to_unicode_string().unwrap(), "안녕");
    }

    #[test]
    fn backend_requests_remote_unicode_text_and_sets_host_clipboard() {
        let (mut backend, runtime, _command_tx, host_rx) = create_text_clipboard_bridge();

        backend.on_remote_copy(&[ironrdp::cliprdr::pdu::ClipboardFormat::new(
            ClipboardFormatId::CF_UNICODETEXT,
        )]);

        let ClipboardMessage::SendInitiatePaste(format) =
            runtime.backend_rx.try_recv().expect("paste request")
        else {
            panic!("expected paste request");
        };
        assert_eq!(format, ClipboardFormatId::CF_UNICODETEXT);

        backend.on_format_data_response(FormatDataResponse::new_unicode_string("from windows"));

        assert_eq!(
            host_rx.try_recv().expect("host clipboard command"),
            HostClipboardCommand::SetText("from windows".to_string())
        );
    }
}
