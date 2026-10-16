use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jint, jmethodID};
use std::sync::{Mutex, OnceLock};

use veloren_client::Client;

/// Stores the Java callback object and method ID for sending events from Rust to Java.
static EVENT_CALLBACK: OnceLock<std::sync::Mutex<Option<EventCallback>>> = OnceLock::new();

struct EventCallback {
    /// Global reference to the Java object implementing the callback interface.
    object: JObject,
    /// Method ID of `onEvent(type: String, data: String)` on that object.
    method_id: jni::sys::jmethodID,
}

unsafe impl Send for EventCallback {}
unsafe impl Sync for EventCallback {}

impl EventCallback {
    pub fn new(object: JObject, method_id: jni::sys::jmethodID) -> Self {
        Self { object, method_id }
    }

    /// Send an event to Java. Must be called from a thread with a valid JNIEnv.
    pub fn send(&self, env: &mut JNIEnv, event_type: &str, data: &str) -> Result<(), jni::errors::Error> {
        let j_type = env.new_string(event_type)?;
        let j_data = env.new_string(data)?;

        env.call_method(
            self.object.as_obj(),
            self.method_id,
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[j_type.into(), j_data.into()],
        )?;

        Ok(())
    }
}

fn event_callback_storage() -> &'static std::sync::Mutex<Option<EventCallback>> {
    EVENT_CALLBACK.get_or_init(|| std::sync::Mutex::new(None))
}

/// JNI: `velorenSetEventCallback(callbackObject)`
/// Registers a Java object to receive events from the Rust client.
/// The method `onEvent(String, String)` is looked up via GetMethodID.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetEventCallback(
    mut env: JNIEnv,
    _class: JClass,
    callback_object: JObject,
) -> jint {
    // Get the class of the callback object
    let callback_class = match env.get_object_class(callback_object) {
        Ok(cls) => cls,
        Err(e) => {
            eprintln!("Failed to get callback object class: {:?}", e);
            return -1;
        }
    };

    // Look up the onEvent(String, String) method ID
    let method_id = match env.get_method_id(
        callback_class,
        "onEvent",
        "(Ljava/lang/String;Ljava/lang/String;)V",
    ) {
        Ok(mid) => mid,
        Err(e) => {
            eprintln!("Failed to get onEvent method ID: {:?}", e);
            return -2;
        }
    };

    // Create a global reference so the object survives beyond the JNI call
    let global_obj = match env.new_global_ref(callback_object) {
        Ok(ref_obj) => ref_obj,
        Err(e) => {
            eprintln!("Failed to create global ref for event callback: {:?}", e);
            return -3;
        }
    };

    let callback_obj: JObject = global_obj.into();
    let callback = EventCallback::new(callback_obj, method_id);
    event_callback_storage().lock().unwrap().replace(callback);

    println!("Event callback registered");
    0
}

/// JNI: `velorenSendEvent(eventType: String, data: String)`
/// Allows Java to send an event back to Rust (e.g., UI button press).
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSendEvent(
    mut env: JNIEnv,
    _class: JClass,
    event_type: JString,
    data: JString,
) -> jint {
    let etype: String = env
        .get_string(event_type)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let edata: String = env
        .get_string(data)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Forward to the client input handler based on event type
    let client_ptr = crate::client_storage().lock().unwrap().as_ref().copied();
    let Some(client_ptr) = client_ptr else {
        return -1;
    };

    match etype.as_str() {
        "chat" => {
            let client = unsafe { &mut *(client_ptr as *mut Client) };
            client.send_chat(edata);
            0
        }
        "command" => {
            let parts: Vec<&str> = edata.split_whitespace().collect();
            if let Some(name) = parts.first() {
                let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
                let client = unsafe { &mut *(client_ptr as *mut Client) };
                client.send_command(name.to_string(), args);
            }
            0
        }
        _ => -1,
    }
}

/// Forward Veloren client events to the Java callback.
/// Call this from `velorenTick` after getting events.
pub fn forward_events(env: &mut JNIEnv, events: &[veloren_client::Event]) {
    let callback = event_callback_storage()
        .lock()
        .unwrap()
        .as_ref();

    let Some(callback) = callback else {
        return;
    };

    for event in events {
        let (etype, data) = match event {
            veloren_client::Event::Chat(msg) => ("chat", format!("{msg:?}")),
            veloren_client::Event::Disconnect => ("disconnect", "".to_string()),
            veloren_client::Event::DisconnectionNotification(code) => {
                ("disconnect_notification", code.to_string())
            }
            veloren_client::Event::InventoryUpdated(items) => {
                ("inventory_updated", format!("{items:?}"))
            }
            veloren_client::Event::Notification(notif) => {
                ("notification", format!("{notif:?}"))
            }
            veloren_client::Event::SetViewDistance(distance) => {
                ("set_view_distance", distance.to_string())
            }
            veloren_client::Event::Outcome(outcome) => ("outcome", format!("{outcome:?}")),
            veloren_client::Event::CharacterCreated(id) => ("character_created", format!("{id:?}")),
            veloren_client::Event::CharacterEdited(id) => ("character_edited", format!("{id:?}")),
            veloren_client::Event::CharacterError(err) => ("character_error", err.clone()),
            veloren_client::Event::StartSpectate(pos) => {
                ("start_spectate", format!("{pos:?}"))
            }
            veloren_client::Event::SpectatePosition(pos) => {
                ("spectate_position", format!("{pos:?}"))
            }
            veloren_client::Event::PluginDataReceived(bytes) => {
                ("plugin_data", format!("{:?}", bytes))
            }
            veloren_client::Event::Gizmos(gizmos) => ("gizmos", format!("{gizmos:?}")),
            veloren_client::Event::InviteComplete { target, answer, kind } => {
                ("invite_complete", format!("{target}:{answer:?}:{kind:?}"))
            }
            veloren_client::Event::TradeComplete { result, trade } => {
                ("trade_complete", format!("{result:?}:{trade:?}"))
            }
            // Events with complex types that may not implement Debug in all contexts
            veloren_client::Event::CharacterJoined(_) => ("character_joined", "".to_string()),
            veloren_client::Event::MapMarker(_) => ("map_marker", "".to_string()),
            veloren_client::Event::Dialogue(_, _) => ("dialogue", "".to_string()),
            veloren_client::Event::GroupInventoryUpdate(_, _) => {
                ("group_inventory_update", "".to_string())
            }
        };

        let _ = callback.send(env, etype, &data);
    }
}
