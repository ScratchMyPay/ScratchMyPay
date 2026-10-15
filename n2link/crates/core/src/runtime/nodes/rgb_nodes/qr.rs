//! `qr-code`: turn text (by default an RGB payment request) into a QR code, offline.
//!
//! Text source, in order: the configured `property`; else `msg.payload` if it is a string;
//! else an `rgb-invoice` output (`payload.invoice.invoice`). Output `format`:
//! `html` (default) a small page ready for an `http response`, or `svg`.

use std::sync::Arc;

use qrcode::QrCode;
use qrcode::render::svg;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::N2linkError;
use crate::runtime::flow::Flow;
use crate::runtime::model::json::RedFlowNodeConfig;
use crate::runtime::model::{MsgHandle, Variant};
use crate::runtime::nodes::*;
use n2link_macro::*;

const MAX_TEXT_CHARS: usize = 2_000;

#[derive(Debug, Deserialize)]
struct QrConfig {
    #[serde(default)]
    property: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    caption: String,
}

crate::node_hints!("qr-code", input = "string|object", outs = ["qr" => "string"]);

#[flow_node("qr-code", red_name = "qr-code", inputs = 1, outputs = 1)]
struct QrCodeNode {
    base: BaseFlowNodeState,
    property: String,
    html: bool,
    caption: String,
}

/// SVG for `text`; errors if the text cannot fit in a QR code.
pub(crate) fn qr_svg(text: &str) -> crate::Result<String> {
    let code = QrCode::new(text.as_bytes())
        .map_err(|err| N2linkError::invalid_operation(&format!("qr-code: text does not fit in a QR code: {err}")))?;
    Ok(code.render::<svg::Color<'_>>().min_dimensions(280, 280).quiet_zone(true).build())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A self-contained page: QR, caption, and the text itself for copy/paste.
pub(crate) fn qr_page(text: &str, caption: &str) -> crate::Result<String> {
    let svg = qr_svg(text)?;
    let caption = if caption.trim().is_empty() { "Scan to pay" } else { caption };
    Ok(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{c}</title><style>body{{font:16px system-ui,sans-serif;margin:24px;text-align:center;background:#fff;color:#111}}\
code{{display:block;word-break:break-all;font-size:12px;background:#f3f1ea;padding:10px;border-radius:8px;text-align:left;margin:16px auto;max-width:520px}}\
svg{{max-width:320px;height:auto}}</style></head><body><h1>{c}</h1>{svg}<code>{t}</code></body></html>",
        c = escape(caption),
        t = escape(text),
    ))
}

fn pick_text(msg: &crate::runtime::model::Msg, property: &str) -> Option<String> {
    if !property.trim().is_empty() {
        return msg.get_nav(property.trim()).and_then(Variant::as_str).map(str::to_owned);
    }
    let payload = msg.get("payload")?;
    if let Some(text) = payload.as_str() {
        return Some(text.to_owned());
    }
    let value = serde_json::to_value(payload).ok()?;
    value.pointer("/invoice/invoice").or_else(|| value.get("invoice")).and_then(Value::as_str).map(str::to_owned)
}

impl QrCodeNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = QrConfig::deserialize(&config.rest)?;
        let html = match raw.format.trim() {
            "" | "html" => true,
            "svg" => false,
            other => {
                return Err(N2linkError::invalid_operation(&format!("qr-code format '{other}' must be html or svg")));
            }
        };
        Ok(Box::new(QrCodeNode { base, property: raw.property, html, caption: raw.caption }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let text = pick_text(&*msg.read().await, &self.property)
            .filter(|t| !t.is_empty() && t.chars().count() <= MAX_TEXT_CHARS)
            .ok_or_else(|| {
                N2linkError::invalid_operation(
                    "qr-code: no text to encode (expected a string or an rgb-invoice result)",
                )
            })?;
        let (body, content_type) = if self.html {
            (qr_page(&text, &self.caption)?, "text/html; charset=utf-8")
        } else {
            (qr_svg(&text)?, "image/svg+xml")
        };
        {
            let mut guard = msg.write().await;
            guard.set("payload".to_owned(), Variant::String(body));
            guard.set("headers".to_owned(), Variant::from(json!({ "content-type": content_type })));
        }
        self.fan_out_one(Envelope { port: 0, msg }, cancel).await
    }
}

#[async_trait]
impl FlowNodeBehavior for QrCodeNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move { node.handle(msg, cancel).await })
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{qr_page, qr_svg};

    #[test]
    fn renders_an_rgb_invoice_as_svg_and_page() {
        let invoice = "rgb:~/~/~/sbc:wvout:Bf8wx12q-GH4YnPq-~f6NfuW-_LaKCGW-0OlaLRl-koSqQMC-LeOPjdA?expiry=1792092077";
        let svg = qr_svg(invoice).unwrap();
        assert!(svg.starts_with("<?xml") || svg.contains("<svg"));
        let page = qr_page(invoice, "Pay the <sound> engineer").unwrap();
        assert!(page.contains("<svg") && page.contains(invoice) && page.contains("&lt;sound&gt;"));
    }

    #[test]
    fn oversized_text_is_an_error() {
        assert!(qr_svg(&"x".repeat(8_000)).is_err());
    }
}
