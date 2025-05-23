// src/lib.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub username: String,
    pub text: String,
    pub timestamp: Option<String>, // Server mungkin menambahkan ini
}

use yew::prelude::*;
use gloo_net::websocket::{futures::WebSocket, Message as WsMessage, WebSocketError};
use wasm_bindgen_futures::spawn_local;
use futures_util::{StreamExt, SinkExt, stream::SplitSink, stream::SplitStream};
use web_sys::HtmlInputElement; // Untuk mendapatkan nilai dari input field

const WEBSOCKET_URL: &str = "ws://127.0.0.1:8080/ws"; // Ganti dengan URL server JS Anda

pub enum Msg {
    Connect, // Pesan untuk memulai koneksi WebSocket
    SetWsWrite(Option<SplitSink<WebSocket, WsMessage>>), // Menyimpan bagian tulis dari WebSocket
    SetWsRead(Option<SplitStream<WebSocket>>), // Menyimpan bagian baca (disimpan untuk referensi, tapi task akan membacanya)
    WsReadTaskStarted, // Konfirmasi task pembacaan WS telah dimulai
    ConnectionFailed,
    MessageReceived(ChatMessage),
    UpdateInput(String),
    SendMessage,
    SetUsername(String),
    UpdateUsernameInput(String),
    Error(String), // Untuk menampilkan error umum
}

pub struct App {
    username: String,
    username_input: String,
    ws_write: Option<SplitSink<WebSocket, WsMessage>>,
    messages: Vec<ChatMessage>,
    current_input: String,
    error: Option<String>,
    is_connected: bool,
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_message(Msg::Connect); // Memulai koneksi saat komponen dibuat
        Self {
            username: String::from("Anonim"), // Default username
            username_input: String::new(),
            ws_write: None,
            messages: Vec::new(),
            current_input: String::new(),
            error: None,
            is_connected: false,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Connect => {
                let link = ctx.link().clone();
                spawn_local(async move {
                    match WebSocket::open(WEBSOCKET_URL) {
                        Ok(ws_conn) => {
                            link.send_message(Msg::SetWsWrite(Some(ws_conn.split().0))); // Kirim bagian tulis
                            link.send_message(Msg::SetWsRead(Some(ws_conn.split().1))); // Kirim bagian baca
                        }
                        Err(e) => {
                            link.send_message(Msg::Error(format!("Gagal terhubung ke WebSocket: {:?}", e)));
                            link.send_message(Msg::ConnectionFailed);
                        }
                    }
                });
                false // Tidak perlu re-render UI segera
            }
            Msg::SetWsWrite(ws_write_half) => {
                self.ws_write = ws_write_half;
                self.is_connected = self.ws_write.is_some();
                self.error = None; // Hapus error jika koneksi berhasil
                true // Re-render untuk update status koneksi
            }
            Msg::SetWsRead(Some(ws_read_half)) => {
                // Mulai task baru untuk membaca pesan dari WebSocket
                let link = ctx.link().clone();
                spawn_local(async move {
                    let mut read_stream = ws_read_half;
                    link.send_message(Msg::WsReadTaskStarted); // Konfirmasi task dimulai
                    while let Some(msg_result) = read_stream.next().await {
                        match msg_result {
                            Ok(WsMessage::Text(text_data)) => {
                                match serde_json::from_str::<ChatMessage>(&text_data) {
                                    Ok(chat_msg) => {
                                        link.send_message(Msg::MessageReceived(chat_msg));
                                    }
                                    Err(e) => {
                                        link.send_message(Msg::Error(format!("Gagal parse pesan server: {}. Data: {}",e, text_data)));
                                    }
                                }
                            }
                            Ok(WsMessage::Bytes(_)) => {
                                link.send_message(Msg::Error("Menerima pesan biner, tidak didukung.".to_string()));
                            }
                            Err(e) => {
                                let err_msg = match e {
                                    WebSocketError::ConnectionError => "Koneksi WebSocket error.".to_string(),
                                    WebSocketError::ConnectionClose(close_event) => format!("Koneksi WebSocket ditutup: code={}, reason='{}'", close_event.code(), close_event.reason()),
                                    WebSocketError::MessageSendError(_) => "Error mengirim pesan WebSocket.".to_string(), // Seharusnya tidak terjadi di read loop
                                    _ => "Error WebSocket tidak diketahui.".to_string(),
                                };
                                link.send_message(Msg::Error(err_msg));
                                link.send_message(Msg::ConnectionFailed); // Set status koneksi gagal
                                break; // Keluar dari loop pembacaan
                            }
                        }
                    }
                    // Jika loop berakhir, berarti koneksi tertutup dari sisi server atau ada error
                    link.send_message(Msg::Error("Koneksi WebSocket terputus.".to_string()));
                    link.send_message(Msg::ConnectionFailed);
                });
                false // Tidak perlu re-render UI segera karena task berjalan di background
            }
            Msg::SetWsRead(None) => { /* Seharusnya tidak terjadi jika SetWsWrite berhasil */ false }
            Msg::WsReadTaskStarted => {
                log::info!("Task pembacaan WebSocket telah dimulai.");
                false
            }
            Msg::ConnectionFailed => {
                self.is_connected = false;
                self.ws_write = None; // Reset write stream
                true // Re-render untuk update status koneksi
            }
            Msg::MessageReceived(msg) => {
                self.messages.push(msg);
                true // Re-render UI untuk menampilkan pesan baru
            }
            Msg::UpdateInput(input) => {
                self.current_input = input;
                false // Tidak perlu re-render untuk setiap ketikan
            }
            Msg::SendMessage => {
                if let Some(ws_write) = &mut self.ws_write {
                    if !self.current_input.is_empty() {
                        let msg_to_send = ChatMessage {
                            username: self.username.clone(),
                            text: self.current_input.clone(),
                            timestamp: None, // Server mungkin yang akan mengisi ini
                        };
                        match serde_json::to_string(&msg_to_send) {
                            Ok(json_msg) => {
                                let current_input_for_log = self.current_input.clone(); // Clone sebelum di-clear
                                let link = ctx.link().clone(); // Clone link untuk task
                                let ws_write_clone = ws_write; // Ini tricky, cara aman adalah tidak menyimpan ws_write di self secara mutlak atau pakai Rc<RefCell<>>
                                                              // Untuk contoh ini, kita spawn task baru dan berharap ws_write masih valid
                                                              // Dalam aplikasi riil, penanganan state koneksi WS perlu lebih robust
                                // Untuk gloo-net, send adalah async, jadi perlu spawn_local
                                let future = ws_write_clone.send(WsMessage::Text(json_msg));
                                spawn_local(async move {
                                    if let Err(e) = future.await {
                                         link.send_message(Msg::Error(format!("Gagal mengirim pesan: {:?}", e)));
                                    } else {
                                         log::info!("Pesan terkirim: {}", current_input_for_log);
                                    }
                                });
                            }
                            Err(e) => {
                                self.error = Some(format!("Gagal serialisasi pesan: {}", e));
                            }
                        }
                        self.current_input.clear();
                    }
                } else {
                    self.error = Some("Tidak terhubung ke server WebSocket.".to_string());
                }
                true // Re-render untuk membersihkan input atau menampilkan error
            }
            Msg::UpdateUsernameInput(input) => {
                self.username_input = input;
                false
            }
            Msg::SetUsername => {
                if !self.username_input.is_empty() {
                    self.username = self.username_input.clone();
                    self.username_input.clear();
                }
                true // Re-render untuk update tampilan username
            }
            Msg::Error(err_msg) => {
                self.error = Some(err_msg);
                log::error!("Error: {:?}", self.error);
                true // Re-render untuk menampilkan error
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();

        let on_input_change = link.callback(|e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            Msg::UpdateInput(input.value())
        });

        let on_username_input_change = link.callback(|e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            Msg::UpdateUsernameInput(input.value())
        });

        let on_send_click = link.callback(|_| Msg::SendMessage);
        let on_set_username_click = link.callback(|_| Msg::SetUsername);

        let on_submit = link.batch_callback(|e: FocusEvent| { // Menggunakan FocusEvent untuk onsubmit form
            e.prevent_default(); // Mencegah reload halaman default
            Some(Msg::SendMessage)
        });
         let on_username_submit = link.batch_callback(|e: FocusEvent| {
            e.prevent_default();
            Some(Msg::SetUsername)
        });


        html! {
            <div class="chat-container">
                <h2>{ "Yew WebChat" }</h2>
                <div>
                    <p>{ format!("Username saat ini: {}", self.username) }</p>
                    if !self.is_connected {
                         <p style="color: red;">{ "Tidak terhubung ke server. Mencoba menghubungkan..." }</p>
                         <button onclick={link.callback(|_| Msg::Connect)}>{ "Coba Hubungkan Ulang" }</button>
                    } else {
                         <p style="color: green;">{ "Terhubung ke server!" }</p>
                    }
                    {
                        if let Some(err) = &self.error {
                            html! { <p style="color: red;">{ format!("Error: {}", err) }</p> }
                        } else {
                            html! {}
                        }
                    }
                </div>
                <div class="username-area">
                    <form onsubmit={on_username_submit}> // Tambahkan form untuk submit username dengan Enter
                        <input
                            type="text"
                            placeholder="Set username..."
                            value={self.username_input.clone()}
                            oninput={on_username_input_change}
                        />
                        <button onclick={on_set_username_click} disabled={self.username_input.is_empty()}>{ "Set Username" }</button>
                    </form>
                </div>

                <ul class="messages">
                    { for self.messages.iter().map(|msg| self.view_message(msg)) }
                </ul>

                <div class="input-area">
                     <form onsubmit={on_submit} style="display: contents;"> // Tambahkan form untuk submit pesan dengan Enter
                        <input
                            type="text"
                            placeholder="Ketik pesan..."
                            value={self.current_input.clone()}
                            oninput={on_input_change}
                            disabled={!self.is_connected}
                        />
                        <button onclick={on_send_click} disabled={self.current_input.is_empty() || !self.is_connected}>
                            { "Kirim" }
                        </button>
                    </form>
                </div>
            </div>
        }
    }
}

// Metode helper untuk merender satu pesan
impl App {
    fn view_message(&self, msg: &ChatMessage) -> Html {
        let is_me = msg.username == self.username;
        let class_name = if is_me { "me" } else { "other" };
        html! {
            <li class={class_name}>
                <div class="message-meta">
                    <strong>{ &msg.username }</strong>
                    {
                        if let Some(ts) = &msg.timestamp {
                            html!{ <span class="timestamp">{ format!(" - {}", ts) }</span> }
                        } else {
                            html!{}
                        }
                    }
                </div>
                <div>{ &msg.text }</div>
            </li>
        }
    }
}


// Fungsi utama untuk menjalankan aplikasi Yew
#[wasm_bindgen(start)]
pub fn run_app() {
    // Inisialisasi logger (opsional, tapi berguna untuk debug)
    // Anda mungkin perlu menambahkan dependensi `wasm-logger` dan `log`
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<App>::new().render();
}