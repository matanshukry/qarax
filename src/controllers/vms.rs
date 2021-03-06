use super::*;
use crate::database::DbConnection;
use crate::models::vm::NewVm;
use crate::services::Backend;

use rocket_contrib::json::{Json, JsonValue};
use rocket_contrib::uuid::Uuid;

#[get("/")]
pub fn index(backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.vm_service.get_all(&conn) {
        Ok(vms) => ApiResponse {
            response: json!({ "vms": vms }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({"error": e.to_string()}),
            status: Status::BadRequest,
        },
    }
}

#[get("/<id>")]
pub fn by_id(id: Uuid, backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.vm_service.get_by_id(&id.to_string(), &conn) {
        Ok(vm) => ApiResponse {
            response: json!({ "vm": vm }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({"error": e.to_string()}),
            status: Status::BadRequest,
        },
    }
}

#[post("/", format = "json", data = "<vm>")]
pub fn add_vm(vm: Json<NewVm>, backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    match backend.vm_service.add_vm(&vm.into_inner(), &conn) {
        Ok(id) => ApiResponse {
            response: json!({ "vm_id": id }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "error": e.to_string() }),
            status: Status::BadRequest,
        },
    }
}

#[post("/<id>/start")]
pub fn start_vm(id: Uuid, backend: State<Backend>, conn: DbConnection) -> JsonValue {
    match backend.vm_service.start(&id.to_string(), &conn) {
        Ok(id) => json!({ "vm_id": id }),
        Err(e) => json!({ "error": format!("could not start vm: {}", e) }),
    }
}

#[post("/<id>/stop")]
pub fn stop_vm(id: Uuid, backend: State<Backend>, conn: DbConnection) -> JsonValue {
    match backend.vm_service.stop(&id.to_string(), &conn) {
        Ok(id) => json!({ "vm_id": id }),
        Err(_) => json!({ "error": "could not stop vm" }),
    }
}

#[post("/<vm_id>/drives/<drive_id>/attach")]
pub fn attach_drive(
    vm_id: Uuid,
    drive_id: Uuid,
    backend: State<Backend>,
    conn: DbConnection,
) -> ApiResponse {
    match backend
        .vm_service
        .attach_drive(vm_id.to_string(), drive_id.to_string(), &conn)
    {
        Ok(_) => ApiResponse {
            response: json!({ "status": "ok" }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "error": e.to_string() }),
            status: Status::BadRequest,
        },
    }
}

#[get("/<vm_id>/drives")]
pub fn drives_for_vm(vm_id: Uuid, backend: State<Backend>, conn: DbConnection) -> ApiResponse {
    let vm = match backend.vm_service.get_by_id(&vm_id.to_string(), &conn) {
        Ok(v) => v,
        Err(e) => {
            return ApiResponse {
                response: json!({ "error": e.to_string() }),
                status: Status::BadRequest,
            }
        }
    };
    match backend.drive_service.get_drives_for_vms(&vm, &conn) {
        Ok(drives) => ApiResponse {
            response: json!({ "drives": drives }),
            status: Status::Ok,
        },
        Err(e) => ApiResponse {
            response: json!({ "error": e.to_string() }),
            status: Status::BadRequest,
        },
    }
}

pub fn routes() -> Vec<rocket::Route> {
    routes![
        index,
        by_id,
        add_vm,
        start_vm,
        stop_vm,
        attach_drive,
        drives_for_vm
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create_backend;
    use crate::models::vm::{NetworkMode, NewVm};

    use rocket::http::ContentType;
    use rocket::local::Client;
    use serde_json::Value;

    embed_migrations!();

    fn get_client() -> (Client, DbConnection) {
        let rocket = rocket::ignite()
            .manage(create_backend())
            .attach(DbConnection::fairing())
            .mount("/vms", routes());

        let conn = DbConnection::get_one(&rocket).expect("Database connection failed");
        embedded_migrations::run(&*conn).expect("Failed to run migrations");
        let client = Client::new(rocket).expect("Failed to get client");
        (client, conn)
    }

    fn create_kernel(backend: &Backend, conn: &DbConnection) -> Result<uuid::Uuid> {
        use crate::models::kernel::NewKernel;
        use crate::models::storage::{NewStorage, StorageConfig, StorageType};

        let sc = StorageConfig {
            host_id: Some(uuid::Uuid::new_v4()),
            path: Some(String::from("/var/storage")),
            pool_name: None,
        };

        let ns = NewStorage {
            name: String::from("dummy"),
            storage_type: StorageType::Local,
            config: sc,
        };

        let storage = backend.storage_service.add(&ns, conn)?;

        let nk = NewKernel {
            name: String::from("linux57"),
            storage_id: storage,
        };

        let kernel_id: uuid::Uuid = backend.kernel_service.add(&nk, conn)?;

        Ok(kernel_id)
    }

    fn create_payload(
        kernel_id: uuid::Uuid,
        network_mode: Option<NetworkMode>,
        address: Option<String>,
        kernel_params: Option<String>,
    ) -> Result<String> {
        let vm = NewVm {
            name: String::from("vm1"),
            vcpu: 1,
            memory: 128,
            kernel: kernel_id,
            network_mode,
            address,
            kernel_params,
        };

        Ok(serde_json::to_string(&vm)?)
    }

    #[test]
    fn test_index_empty() {
        let (client, _) = get_client();
        let mut response = client.get("/vms").dispatch();

        let response = response.body_string();
        let response: Value = serde_json::from_str(&response.unwrap()).unwrap();

        assert_eq!(response.to_string(), "{\"vms\":[]}");
    }

    #[test]
    fn test_add_vm_no_network() {
        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();
        let kernel_id = create_kernel(&backend, &conn).unwrap();
        let payload = create_payload(kernel_id, None, None, None).unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).unwrap().len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(vm.network_mode, None);

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
        backend.kernel_service.delete_all(&conn).unwrap();
        backend.storage_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_add_vm_dhcp_network() {
        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();
        let kernel_id = create_kernel(&backend, &conn).unwrap();
        let payload = create_payload(kernel_id, Some(NetworkMode::Dhcp), None, None).unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).unwrap().len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(vm.network_mode, Some(String::from("dhcp")));

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
        backend.kernel_service.delete_all(&conn).unwrap();
        backend.storage_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_add_vm_static_ip_network() {
        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();
        let kernel_id = create_kernel(&backend, &conn).unwrap();
        let payload = create_payload(
            kernel_id,
            Some(NetworkMode::StaticIp),
            Some(String::from("192.168.122.100")),
            None,
        )
        .unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).unwrap().len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(vm.network_mode, Some(String::from("static_ip")));
        assert_eq!(vm.address, Some(String::from("192.168.122.100")));

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
        backend.kernel_service.delete_all(&conn).unwrap();
        backend.storage_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_default_kernel_params() {
        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();
        let kernel_id = create_kernel(&backend, &conn).unwrap();
        let payload = create_payload(kernel_id, None, None, None).unwrap();
        println!("{}", payload);
        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).unwrap().len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(
            vm.kernel_params,
            String::from("console=ttyS0 reboot=k panic=1 pci=off")
        );

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
        backend.kernel_service.delete_all(&conn).unwrap();
        backend.storage_service.delete_all(&conn).unwrap();
    }

    #[test]
    fn test_custom_kernel_params() {
        let (client, conn) = get_client();
        let backend: State<Backend> = State::from(&client.rocket()).unwrap();
        let kernel_id = create_kernel(&backend, &conn).unwrap();
        let payload = create_payload(
            kernel_id,
            Some(NetworkMode::StaticIp),
            Some(String::from("192.168.122.100")),
            Some(String::from("ip=1.1.1.1")),
        )
        .unwrap();

        let mut response = client
            .post("/vms")
            .header(ContentType::JSON)
            .body(payload)
            .dispatch();

        let response: Value = serde_json::from_str(&response.body_string().unwrap()).unwrap();
        let vm_id = response["vm_id"].as_str().unwrap();

        assert_eq!(backend.vm_service.get_all(&conn).unwrap().len(), 1);

        let vm = backend.vm_service.get_by_id(vm_id, &conn).unwrap();
        assert_eq!(vm.kernel_params, String::from("ip=1.1.1.1"));

        // TODO: Stupid teardown
        backend.vm_service.delete_all(&conn).unwrap();
        backend.kernel_service.delete_all(&conn).unwrap();
        backend.storage_service.delete_all(&conn).unwrap();
    }
}
