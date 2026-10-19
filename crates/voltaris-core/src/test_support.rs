use nng::options::Options;
use prost::Message;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use voltaris_ipc::gen::kiapi;

/// An already-listening in-process KiCad IPC double.
///
/// The guard owns the endpoint for its entire lifetime. Dropping it closes the
/// shared NNG socket, wakes the receive loop, joins the worker, and releases
/// the endpoint name. Tests therefore never probe and relinquish a TCP port or
/// leave a detached mock thread behind.
pub(crate) struct MockIpcServer {
    address: String,
    control: nng::Socket,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl MockIpcServer {
    pub(crate) fn spawn(
        purpose: &str,
        respond: impl Fn(kiapi::common::ApiRequest) -> kiapi::common::ApiResponse + Send + 'static,
    ) -> Self {
        static NEXT_SERVER: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT_SERVER.fetch_add(1, Ordering::Relaxed);
        Self::spawn_at(
            format!(
                "inproc://voltaris-core-{}-{sequence}-{purpose}",
                std::process::id()
            ),
            respond,
        )
    }

    /// As [`Self::spawn`], on an endpoint the caller names.
    ///
    /// Lets one test replace the KiCad behind a *running* server without
    /// rebuilding it — the only way to exercise a session whose editor changes
    /// under it, since the endpoint is fixed when the server is configured.
    /// The previous guard must be dropped first; it releases the name.
    pub(crate) fn spawn_at(
        address: String,
        respond: impl Fn(kiapi::common::ApiRequest) -> kiapi::common::ApiResponse + Send + 'static,
    ) -> Self {
        let socket = nng::Socket::new(nng::Protocol::Rep0).expect("mock rep socket");
        socket
            .set_opt::<nng::options::RecvTimeout>(Some(Duration::from_secs(10)))
            .expect("mock receive timeout");
        socket.listen(&address).expect("mock listen");

        let control = socket.clone();
        let worker = std::thread::spawn(move || {
            while let Ok(message) = socket.recv() {
                let Ok(request) = kiapi::common::ApiRequest::decode(message.as_slice()) else {
                    break;
                };
                let response = respond(request);
                let output = nng::Message::from(response.encode_to_vec().as_slice());
                if socket.send(output).is_err() {
                    break;
                }
            }
        });

        Self {
            address,
            control,
            worker: Some(worker),
        }
    }

    pub(crate) fn address(&self) -> &str {
        &self.address
    }
}

impl Drop for MockIpcServer {
    fn drop(&mut self) {
        self.control.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nng::options::Options;
    use std::collections::HashSet;

    fn ok_response() -> kiapi::common::ApiResponse {
        kiapi::common::ApiResponse {
            status: Some(kiapi::common::ApiResponseStatus {
                status: kiapi::common::ApiStatusCode::AsOk as i32,
                error_message: String::new(),
            }),
            header: None,
            message: None,
        }
    }

    fn round_trip(server: &MockIpcServer) {
        let socket = nng::Socket::new(nng::Protocol::Req0).expect("mock client socket");
        socket
            .set_opt::<nng::options::RecvTimeout>(Some(Duration::from_secs(1)))
            .expect("client timeout");
        socket.dial(server.address()).expect("server is ready");
        let request = kiapi::common::ApiRequest {
            header: None,
            message: None,
        };
        socket
            .send(nng::Message::from(request.encode_to_vec().as_slice()))
            .expect("request");
        let response = socket.recv().expect("response");
        let response = kiapi::common::ApiResponse::decode(response.as_slice()).expect("decode");
        assert_eq!(
            response.status.expect("status").status,
            kiapi::common::ApiStatusCode::AsOk as i32
        );
    }

    #[test]
    fn returned_server_is_already_bound_and_responsive() {
        let server = MockIpcServer::spawn("ready", |_| ok_response());
        round_trip(&server);
    }

    #[test]
    fn concurrent_servers_own_distinct_endpoints() {
        let servers: Vec<_> = (0..64)
            .map(|_| MockIpcServer::spawn("parallel", |_| ok_response()))
            .collect();
        let addresses: HashSet<_> = servers.iter().map(MockIpcServer::address).collect();
        assert_eq!(addresses.len(), servers.len());
        for server in &servers {
            round_trip(server);
        }
    }

    #[test]
    fn dropping_the_guard_closes_and_releases_its_endpoint() {
        let server = MockIpcServer::spawn("release", |_| ok_response());
        let address = server.address().to_string();
        drop(server);

        let replacement = nng::Socket::new(nng::Protocol::Rep0).expect("replacement socket");
        replacement
            .listen(&address)
            .expect("the dropped guard releases the in-process endpoint");
    }

    #[test]
    fn responder_failure_still_releases_the_endpoint() {
        let server = MockIpcServer::spawn("failed-response", |_| panic!("mock responder failed"));
        let address = server.address().to_string();
        let socket = nng::Socket::new(nng::Protocol::Req0).expect("mock client socket");
        socket
            .set_opt::<nng::options::RecvTimeout>(Some(Duration::from_secs(1)))
            .expect("client timeout");
        socket.dial(server.address()).expect("server is ready");
        let request = kiapi::common::ApiRequest {
            header: None,
            message: None,
        };
        socket
            .send(nng::Message::from(request.encode_to_vec().as_slice()))
            .expect("request");
        assert!(socket.recv().is_err(), "failed responder must not reply");
        drop(socket);
        drop(server);

        let replacement = nng::Socket::new(nng::Protocol::Rep0).expect("replacement socket");
        replacement
            .listen(&address)
            .expect("a failed worker must not leak its endpoint");
    }

    #[test]
    fn repeated_fixture_lifecycles_remain_ready_and_releasable() {
        for _ in 0..32 {
            let server = MockIpcServer::spawn("repeated", |_| ok_response());
            round_trip(&server);
            let address = server.address().to_string();
            drop(server);

            let replacement = nng::Socket::new(nng::Protocol::Rep0).expect("replacement socket");
            replacement
                .listen(&address)
                .expect("each fixture lifecycle releases its endpoint");
        }
    }
}
