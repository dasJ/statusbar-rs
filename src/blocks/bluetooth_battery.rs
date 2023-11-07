//! Lists bluetooth devices with batteries and their state

use std::collections::HashMap;
use std::sync::{mpsc::Sender, Arc, RwLock};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant;

pub struct BluetoothBattery {
    dbus_conn: Arc<Connection>,
    devices: Arc<RwLock<HashMap<zvariant::OwnedObjectPath, Device>>>,
}

#[derive(Debug)]
struct Device {
    percentage: u8,
    icon: Option<String>,
}

impl BluetoothBattery {
    pub fn new(timer_cancel: &Sender<()>) -> Option<Self> {
        // Connect
        let Ok(dbus_conn) = Connection::system() else {
            return None;
        };
        let dbus_conn = Arc::new(dbus_conn);

        // Build proxy
        let Ok(object_manager) = Proxy::new(
            &dbus_conn,
            "org.bluez",
            "/",
            "org.freedesktop.DBus.ObjectManager",
        ) else {
            return None;
        };

        let devices = Arc::new(RwLock::new(
            HashMap::<zvariant::OwnedObjectPath, Device>::new(),
        ));

        // Handler for added devices
        let stream = object_manager.receive_signal("InterfacesAdded").ok()?;
        let conn = Arc::clone(&dbus_conn);
        let devs = Arc::clone(&devices);
        let sender = timer_cancel.clone();
        std::thread::spawn(move || {
            for item in stream {
                // Deconstruct the body of the signal
                // This also gets us the battery percentage and skips devices without battery
                let body: zbus::zvariant::Structure = match item.body() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let zvariant::Value::ObjectPath(path) = body.fields()[0].clone() else {
                    continue;
                };
                let zvariant::Value::Dict(rest) = body.fields()[1].clone() else {
                    continue;
                };
                let Ok(Some(zvariant::Value::Dict(batt))) = rest.get("org.bluez.Battery1") else {
                    continue;
                };
                let Ok(Some(zvariant::Value::U8(percentage))) = batt.get("Percentage") else {
                    continue;
                };

                // Ask for the icon
                let Ok(proxy) = Proxy::new(&conn, "org.bluez", &path, "org.bluez.Device1") else {
                    continue;
                };
                let icon = proxy.get_property::<String>("Icon").ok();

                // Insert the device
                let dev = Device {
                    percentage: *percentage,
                    icon,
                };
                devs.write().unwrap().insert(path.into_owned().into(), dev);
                let _idc = sender.send(());
            }
        });

        // Handler for removed devices
        let stream = object_manager.receive_signal("InterfacesRemoved").ok()?;
        let devs = Arc::clone(&devices);
        let sender = timer_cancel.clone();
        std::thread::spawn(move || {
            for item in stream {
                let body: zbus::zvariant::Structure = match item.body() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let zvariant::Value::ObjectPath(path) = body.fields()[0].clone() else {
                    continue;
                };
                devs.write().unwrap().remove(&path.into_owned().into());
                let _idc = sender.send(());
            }
        });

        // Query initial state
        let objects: HashMap<
            zvariant::OwnedObjectPath,
            HashMap<String, HashMap<String, zvariant::OwnedValue>>,
        > = object_manager.call("GetManagedObjects", &()).ok()?;
        for (path, obj) in objects {
            let Some(bat) = obj.get("org.bluez.Battery1") else {
                continue;
            };
            let Some(percentage) = bat.get("Percentage") else {
                continue;
            };
            let zvariant::Value::U8(percentage) = &**percentage else {
                continue;
            };

            // Ask for the icon
            let Ok(proxy) = Proxy::new(&dbus_conn, "org.bluez", &path, "org.bluez.Device1") else {
                continue;
            };
            let icon = proxy.get_property::<String>("Icon").ok();

            // Insert the device
            let dev = Device {
                percentage: *percentage,
                icon,
            };
            devices.write().unwrap().insert(path.clone(), dev);
        }

        Some(Self { dbus_conn, devices })
    }

    /// Updates all percentages
    pub fn update(&self) {
        let dbus_conn = Arc::clone(&self.dbus_conn);
        let devices = Arc::clone(&self.devices);
        std::thread::spawn(move || {
            for (path, dev) in &mut *devices.write().unwrap() {
                let Ok(proxy) = Proxy::new(&dbus_conn, "org.bluez", path, "org.bluez.Battery1")
                else {
                    continue;
                };
                dev.percentage = proxy.get_property::<u8>("Percentage").unwrap_or_default();
            }
        });
    }

    /// Returns all icons and percentages
    pub fn percentages(&self) -> Vec<(Option<String>, u8)> {
        self.devices
            .read()
            .unwrap()
            .values()
            .map(|dev| (dev.icon.clone(), dev.percentage))
            .collect()
    }
}
