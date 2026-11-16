// 4개 앱(dogroo·donoti·docook·doquiz) 공통 사본 - 고칠 때 네 곳을 같이 고친다.
// multipart·static_files 는 dogroo·docook, webpush 는 dogroo·donoti 에만 있다.
pub mod app;
pub mod config;
pub mod dates;
pub mod db;
pub mod js;
pub mod password;
pub mod session;
pub mod webpush;
