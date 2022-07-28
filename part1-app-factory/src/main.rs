use std::sync::{atomic::AtomicUsize, Arc};

use crate::{
    http::ConnInfo,
    util::{app_factory_fn, app_fn},
};

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

    #[derive(Clone, Debug)]
    pub struct ConnInfo {
        pub host_and_port: String,
    }
}

mod fakeserver {
    use std::collections::HashMap;

    use tokio::time::{sleep, Duration};
    use tower::{Service, ServiceExt};

    use crate::http::{ConnInfo, Request, Response};

    pub async fn run<AppFactory, App>(mut app_factory: AppFactory)
    where
        AppFactory: Service<ConnInfo, Response = App>,
        AppFactory::Error: std::fmt::Debug + Send,
        AppFactory::Future: Send + 'static,
        App: Send,
        App: Service<Request, Response = Response>,
        App::Error: std::fmt::Debug,
        App::Future: Send + 'static,
    {
        let mut connect_number = 0;

        loop {
            sleep(Duration::from_secs(2)).await;

            connect_number += 1;
            let conn_info = ConnInfo {
                host_and_port: format!("Fake info, connection #{}", connect_number),
            };

            let app = match app_factory.ready().await {
                Err(e) => {
                    eprintln!("Service not able to accept connection {:?}", e);
                    continue;
                }
                Ok(app) => app,
            };

            let future = app.call(conn_info.clone());

            tokio::spawn(async move {
                match future.await {
                    Ok(app) => {
                        println!("Accepted a connection: {:?}", conn_info);
                        run_iner(app).await;
                    }
                    Err(e) => eprintln!("Error occurred: {:?}", e),
                }
            });
        }
    }

    async fn run_iner<App>(mut app: App)
    where
        App: Service<Request, Response = Response>,
        App::Error: std::fmt::Debug,
        App::Future: Send + 'static,
    {
        loop {
            sleep(Duration::from_secs(1)).await;

            let req = Request {
                path_and_query: "/fake/path?page=1".to_owned(),
                headers: HashMap::new(),
                body: Vec::new(),
            };

            let app = match app.ready().await {
                Err(e) => {
                    eprintln!("Service not able to accept request: {:?}", e);
                    continue;
                }
                Ok(app) => app,
            };

            let future = app.call(req);

            tokio::spawn(async move {
                match future.await {
                    Err(e) => eprintln!("Error occurred {:?}", e),
                    Ok(resp) => println!("Successful response {:?}", resp),
                }
            });
        }
    }
}

mod util {
    use std::future::Future;

    use crate::http::{ConnInfo, Request, Response};
    use anyhow::Error;
    use tower::Service;

    pub struct AppFactoryFn<F> {
        f: F,
    }

    pub fn app_factory_fn<F, Ret, App>(f: F) -> AppFactoryFn<F>
    where
        F: FnMut(ConnInfo) -> Ret,
        Ret: Future<Output = Result<App, Error>>,
    {
        AppFactoryFn { f }
    }

    impl<F, Ret, App> Service<ConnInfo> for AppFactoryFn<F>
    where
        F: FnMut(ConnInfo) -> Ret,
        Ret: Future<Output = Result<App, Error>>,
    {
        type Response = App;
        type Error = Error;
        type Future = Ret;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, conn_info: ConnInfo) -> Self::Future {
            (self.f)(conn_info)
        }
    }

    pub struct AppFn<F> {
        f: F,
    }

    pub fn app_fn<F, Ret>(f: F) -> AppFn<F>
    where
        F: FnMut(Request) -> Ret,
        Ret: Future<Output = Result<Response, Error>>,
    {
        AppFn { f }
    }

    impl<F, Ret> Service<Request> for AppFn<F>
    where
        F: FnMut(Request) -> Ret,
        Ret: Future<Output = Result<Response, Error>>,
    {
        type Response = Response;
        type Error = Error;
        type Future = Ret;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: Request) -> Self::Future {
            (self.f)(req)
        }
    }
}

#[tokio::main]
async fn main() {
    use crate::http::Response;
    let counter = Arc::new(AtomicUsize::new(0));

    let mk_app = |conn: ConnInfo| {
        app_fn(move |mut req| {
            println!("Handling a request: {:?}", req.path_and_query);
            let counter = counter.clone();
            let conn_info = conn.clone();
            async move {
                let counter = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                anyhow::ensure!(counter % 4 != 2, "Failing 25% of the time, just for fun");

                req.headers.insert(
                    format!("Conn: {:?}, X-Counter", conn_info),
                    counter.to_string(),
                );

                let resp = Response {
                    status: 200,
                    headers: req.headers,
                    body: req.body,
                };

                Ok(resp)
            }
        })
    };

    let app_factory = app_factory_fn(|conn| {
        println!("Starting a new app for connection {:?}", conn);
        let app = (mk_app.clone())(conn);
        async move { Ok(app) }
    });

    fakeserver::run(app_factory).await;
}
