use std::sync::{atomic::AtomicUsize, Arc};

use crate::util::app_fn;

mod http {
    use std::collections::HashMap;

    #[derive(Debug)]
    pub struct Request {
        pub path_and_query: String,
        pub headers: HashMap<String, String>,
        pub body: Vec<u8>,
    }

    #[derive(Debug)]
    pub struct Response {
        pub status: u32,
        pub headers: HashMap<String, String>,
        pub body: Vec<u8>,
    }
}

mod fakeserver {
    use std::collections::HashMap;

    use tokio::time::{sleep, Duration};
    use tower::{Service, ServiceExt};

    use crate::http;

    pub async fn run<App>(mut app: App)
    where
        App: Service<http::Request, Response = http::Response>,
        App::Error: std::fmt::Debug,
        App::Future: Send + 'static,
    {
        loop {
            sleep(Duration::from_secs(5)).await;

            let req = http::Request {
                path_and_query: "/fake/path?page=1".to_owned(),
                headers: HashMap::new(),
                body: Vec::new(),
            };

            let app = match app.ready().await {
                Err(e) => {
                    eprintln!("Service not able to access request: {:?}", e);
                    continue;
                }
                Ok(app) => app,
            };

            let future = app.call(req);

            tokio::spawn(async move {
                match future.await {
                    Ok(res) => println!("Successful response {:?}", res),
                    Err(e) => eprintln!("Error occurred {:?}", e),
                }
            });
        }
    }
}

mod util {
    use std::{future::Future, task::Poll};

    use tower::Service;

    use crate::http;

    pub struct AppFn<F> {
        pub f: F,
    }

    pub fn app_fn<F, Ret>(f: F) -> AppFn<F>
    where
        F: FnMut(http::Request) -> Ret,
        Ret: Future<Output = Result<http::Response, anyhow::Error>>,
    {
        AppFn { f }
    }

    impl<F, Ret> Service<http::Request> for AppFn<F>
    where
        F: FnMut(http::Request) -> Ret,
        Ret: Future<Output = Result<http::Response, anyhow::Error>>,
    {
        type Response = http::Response;
        type Error = anyhow::Error;
        type Future = Ret;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request) -> Self::Future {
            (self.f)(req)
        }
    }
}
mod app {
    use std::{
        future::Future,
        pin::Pin,
        sync::{atomic::AtomicUsize, Arc},
    };

    use tower::Service;

    use crate::http::{self, Request};

    pub struct DemoApp {
        pub counter: Arc<AtomicUsize>,
    }

    impl Default for DemoApp {
        fn default() -> Self {
            DemoApp {
                counter: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Service<Request> for DemoApp {
        type Response = http::Response;
        type Error = anyhow::Error;
        type Future = Pin<Box<dyn Future<Output = Result<http::Response, Self::Error>> + Send>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, mut req: Request) -> Self::Future {
            let counter = self.counter.clone();

            Box::pin(async move {
                println!("Handling a request {:?}", req.path_and_query);
                let counter = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                anyhow::ensure!(counter % 4 != 2, "Failing 25% of the time, just for fun");

                req.headers
                    .insert("X-Counter".to_owned(), counter.to_string());

                let resp = http::Response {
                    status: 200,
                    headers: req.headers,
                    body: req.body,
                };

                Ok(resp)
            })
        }
    }
}

#[tokio::main]
async fn main() {
    println!("Fakeserver started");

    let counter = Arc::new(AtomicUsize::new(0));

    fakeserver::run(app_fn(move |mut req| {
        let counter = counter.clone();
        async move {
            println!("Handling a request {:?}", req.path_and_query);
            let counter = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            anyhow::ensure!(counter % 4 != 2, "Failing 25% of the time, just for fun");

            req.headers
                .insert("X-Counter".to_owned(), counter.to_string());

            let resp = http::Response {
                status: 200,
                headers: req.headers,
                body: req.body,
            };

            Ok(resp)
        }
    }))
    .await
}
