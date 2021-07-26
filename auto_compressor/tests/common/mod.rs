use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres::Client;
use postgres_openssl::MakeTlsConnector;

pub static DB_URL: &str = "postgresql://synapse_user:synapse_pass@localhost/synapse";

pub fn empty_database() {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(DB_URL, connector).unwrap();

    // delete all the contents from all three tables
    let mut sql = "".to_string();
    sql.push_str("DELETE FROM state_compressor_state;\n");
    sql.push_str("DELETE FROM state_compressor_progress;\n");

    client.batch_execute(&sql).unwrap();
}
