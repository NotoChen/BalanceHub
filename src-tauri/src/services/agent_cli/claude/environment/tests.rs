use super::parse::strict_transport;
use crate::models::AgentMcpTransport;

mod skills;

#[test]
fn strict_transport_rejects_wrong_types_and_conflicting_fields() {
    assert_eq!(
        strict_transport(&serde_json::json!({
            "command": "node",
            "args": ["server", "--mode", "stdio"],
            "type": "stdio"
        })),
        AgentMcpTransport::Stdio
    );
    assert_eq!(
        strict_transport(&serde_json::json!({
            "command": "node",
            "type": ["stdio"]
        })),
        AgentMcpTransport::Unknown
    );
    assert_eq!(
        strict_transport(&serde_json::json!({
            "url": "https://example.test/mcp",
            "type": "sse",
            "transport": "http"
        })),
        AgentMcpTransport::Unknown
    );
    assert_eq!(
        strict_transport(&serde_json::json!({
            "url": "https://example.test/mcp",
            "args": "not-an-array"
        })),
        AgentMcpTransport::Unknown
    );
}
