use crate::frame::{tcp::*, *};
use crate::proto::tcp::Proto;
use crate::codec::req_to_fn_code;

use futures::Future;
use std::net::SocketAddr;
use tokio_proto::TcpServer;
use tokio_service::{NewService, Service};

struct ServiceWrapper<S> {
    service: S,
}

impl<S> ServiceWrapper<S> {
    fn new(service: S) -> Self {
        Self { service }
    }
}

impl<S> Service for ServiceWrapper<S>
where
    S: Service + Send + Sync + 'static,
    S::Request: From<Request>,
    S::Response: Into<Response>,
    S::Error: Into<MbError>,
{
    type Request = RequestAdu;
    type Response = ResponseAdu;
    type Error = MbError;
    type Future = Box<dyn Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, adu: Self::Request) -> Self::Future {
        let Self::Request { hdr, pdu, .. } = adu;
        let req: Request = pdu.into();
        let req_fn_code = req_to_fn_code(&req);
        Box::new(self.service.call(req.into()).then(move |rsp| match rsp {
            Ok(rsp) => {
                let rsp: Response = rsp.into();
                let pdu = rsp.into();
                Ok(Self::Response { hdr, pdu })
            }
            Err(e) => {
                let mbe: MbError = e.into();
                if let MbError::Exception(eee) = mbe {
                    let er = ExceptionResponse { function: req_fn_code, exception: eee};
                    let pdu = er.into();
                    Ok(Self::Response { hdr, pdu })
                } else {
                    Err(mbe.into())
                }
} 
            
    }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    socket_addr: SocketAddr,
    threads: Option<usize>,
}

impl Server {
    /// Set the address for the server (mandatory).
    pub fn new(socket_addr: SocketAddr) -> Self {
        Self {
            socket_addr,
            threads: None,
        }
    }

    /// Set the number of threads running simultaneous event loops (optional, Unix only).
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = Some(threads);
        self
    }

    /// Start a Modbus TCP server that blocks the current thread.
    pub fn serve<S>(self, service: S)
    where
        S: NewService + Send + Sync + 'static,
        S::Request: From<Request>,
        S::Response: Into<Response>,
        S::Error: Into<MbError>,
        S::Instance: Send + Sync + 'static,
    {
        let mut server = TcpServer::new(Proto, self.socket_addr);
        if let Some(threads) = self.threads {
            server.threads(threads);
        }
        server.serve(move || Ok(ServiceWrapper::new(service.new_service()?)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future;

    #[test]
    fn service_wrapper() {
        #[derive(Clone)]
        struct DummyService {
            response: Response,
        };

        impl Service for DummyService {
            type Request = Request;
            type Response = Response;
            type Error = MbError;
            type Future = Box<dyn Future<Item = Self::Response, Error = Self::Error>>;

            fn call(&self, _: Self::Request) -> Self::Future {
                Box::new(future::ok(self.response.clone()))
            }
        }

        let s = DummyService {
            response: Response::ReadInputRegisters(vec![0x33]),
        };
        let service = ServiceWrapper::new(s.clone());

        let hdr = Header {
            transaction_id: 9,
            unit_id: 7,
        };
        let pdu = Request::ReadInputRegisters(0, 1).into();
        let req_adu = RequestAdu {
            hdr,
            pdu,
            disconnect: false,
        };
        let rsp_adu = service.call(req_adu).wait().unwrap();

        assert_eq!(
            rsp_adu.hdr,
            Header {
                transaction_id: 9,
                unit_id: 7,
            }
        );
        assert_eq!(rsp_adu.pdu, s.response.into());
    }
}
