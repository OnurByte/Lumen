use lumen_debate::provider::{ModelProvider, ModelRequest, OpenAiCompatible};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn streams_split_utf8_and_keeps_agent_identity_and_audience_roles() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 2048];
        loop {
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            if let Some(header_end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if request.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.contains("Ada persona"));
        assert!(request.contains("Character skill sheet"));
        assert!(request.contains("Imza: kayitlara gecsin"));
        assert!(request.contains("audience untrusted"));
        assert!(request.contains("\"stream\":true"));
        assert!(request.contains("\"max_tokens\":64"));
        assert!(request.contains("\"temperature\":"));
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"İyi\"}}]}\n\ndata: [DONE]\n\n";
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
        for byte in body.as_bytes() {
            socket.write_all(&[*byte]).await.unwrap();
        }
    });
    let provider = OpenAiCompatible::new(&endpoint, "test-model", None)
        .unwrap()
        .generation_settings(0.9, 64)
        .unwrap();
    let (tx, mut rx) = mpsc::channel(4);
    provider
        .stream(
            ModelRequest {
                persona: "Ada persona".into(),
                position: "A".into(),
                skill: Some("## Imza: kayitlara gecsin".into()),
                phase: None,
                topic: "Tarih".into(),
                audible_context: vec!["Heard only".into()],
                audience: Some("audience untrusted".into()),
            },
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(rx.recv().await.unwrap(), "İyi");
    assert!(rx.recv().await.is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn cancelled_request_never_contacts_provider() {
    let provider =
        OpenAiCompatible::new("http://127.0.0.1:1/v1/chat/completions", "test", None).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (tx, _) = mpsc::channel(1);
    assert!(
        provider
            .stream(
                ModelRequest {
                    persona: "Ada".into(),
                    position: "A".into(),
                    skill: None,
                    phase: None,
                    topic: "Tarih".into(),
                    audible_context: vec![],
                    audience: None
                },
                tx,
                cancel
            )
            .await
            .is_err()
    );
}

#[test]
fn disallows_plaintext_remote_keys_and_invalid_configuration() {
    assert!(
        OpenAiCompatible::new(
            "http://example.com/v1/chat/completions",
            "m",
            Some("secret".into())
        )
        .is_err()
    );
    assert!(OpenAiCompatible::new("file:///tmp/foo", "m", None).is_err());
    assert!(OpenAiCompatible::new("https://example.com/v1/chat/completions", "", None).is_err());
}

#[tokio::test]
async fn cancellation_interrupts_pending_network_read() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        trigger.cancel();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    });
    let provider = OpenAiCompatible::new(&endpoint, "test", None).unwrap();
    let (tx, _) = mpsc::channel(1);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        provider.stream(
            ModelRequest {
                persona: "Ada".into(),
                position: "A".into(),
                skill: None,
                phase: None,
                topic: "Tarih".into(),
                audible_context: vec![],
                audience: None,
            },
            tx,
            cancel,
        ),
    )
    .await;
    assert!(result.unwrap().is_err());
    server.await.unwrap();
}
