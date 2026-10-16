use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use std::sync::{Mutex, OnceLock};

use veloren_client::Client;

/// Stores the Java callback object for sending events from Rust to Java.
static EVENT_CALLBACK: OnceLock<std::sync::Mutex<Option<jni::objects::GlobalRef>>> = OnceLock::new();

fn event_callback_storage() -> &'static std::sync::Mutex<Option<jni::objects::GlobalRef>> {
    EVENT_CALLBACK.get_or_init(|| std::sync::Mutex::new(None))
}

/// JNI: `velorenSetEventCallback(callbackObject)`
/// Registers a Java object to receive events from the Rust client.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetEventCallback(
    mut env: JNIEnv,
    _class: JClass,
    callback_object: JObject,
) -> i32 {
    let global_obj = match env.new_global_ref(callback_object) {
        Ok(ref_obj) => ref_obj,
        Err(e) => {
            eprintln!("Failed to create global ref for event callback: {:?}", e);
            return -3;
        }
    };

    event_callback_storage().lock().unwrap().replace(global_obj);

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
) -> i32 {
    let etype: String = env
        .get_string(&event_type)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|_| String::new());
    let edata: String = env
        .get_string(&data)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|_| String::new());

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
    let binding = event_callback_storage().lock().unwrap();
    let global_ref = binding.as_ref();

    let Some(global_ref) = global_ref else {
        return;
    };

    for event in events {
        let (etype, data): (&str, String) = match event {
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
            veloren_client::Event::CharacterJoined(_) => ("character_joined", "".to_string()),
            veloren_client::Event::MapMarker(_) => ("map_marker", "".to_string()),
            veloren_client::Event::Dialogue(_, _) => ("dialogue", "".to_string()),
            veloren_client::Event::GroupInventoryUpdate(_, _) => {
                ("group_inventory_update", "".to_string())
            }
        };

        let j_type = env.new_string(etype).unwrap();
        let j_data = env.new_string(&data).unwrap();

        // Look up the method ID each time (simpler than storing it with lifetime issues)
        let class = env.get_object_class(global_ref).unwrap();
        let method_id = env
            .get_method_id(&class, "onEvent", "(Ljava/lang/String;Ljava/lang/String;)V")
            .unwrap();

        let _ = env.call_method(
            global_ref.as_obj(),
            "(Ljava/lang/String;Ljava/lang/String;)V",
            method_id,
            &[jni::objects::JValue::Object(j_type.into_inner()), jni::objects::JValue::Object(j_data.into_inner())],
        );
    }
}
