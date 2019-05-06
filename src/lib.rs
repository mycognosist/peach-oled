#[macro_use]
extern crate log;
extern crate failure;
extern crate linux_embedded_hal as hal;
extern crate ssd1306;
extern crate embedded_graphics;
extern crate validator;
#[macro_use]
extern crate validator_derive;

use std::error::Error as StdError;
use std::result::Result;
use std::sync::{Arc, Mutex};

use failure::Fail;

use hal::I2cdev;

use ssd1306::prelude::*;
use ssd1306::Builder;

use embedded_graphics::coord::Coord;
use embedded_graphics::fonts::*;
use embedded_graphics::prelude::*;

use jsonrpc_http_server::jsonrpc_core::types::error::Error;
use jsonrpc_http_server::jsonrpc_core::*;
use jsonrpc_http_server::*;

use validator::{Validate, ValidationErrors};

use serde::Deserialize;

//define the Msg struct for receiving display write commands
#[derive(Debug, Validate, Deserialize)]
pub struct Msg {
    #[validate(range(min = "0", max = "128", message = "x_coord not in range 0-128"))]
    x_coord: i32,
    #[validate(range(min = "0", max = "57", message = "y_coord not in range 0-57"))]
    y_coord: i32,
    #[validate(length(max = "21", message = "string length > 21 characters"))]
    string: String,
    font_size: String,
}

#[derive(Debug, Fail)]
pub enum WriteError {
    #[fail(display = "validation error")]
    Invalid { e: ValidationErrors },

    #[fail(display = "missing expected parameters")]
    MissingParams { e: Error },
}

impl From<WriteError> for Error {
    fn from(err: WriteError) -> Self {
        match &err {
            WriteError::Invalid { e } => {
                let err_clone = e.clone();
                // extract error from ValidationErrors
                let field_errs = err_clone.field_errors();
                let checks = vec!["x_coord", "y_coord", "string"];
                // check source of validation err
                for &error in &checks {
                    let validation_err = field_errs.get(&error);
                    if validation_err.is_some() {
                        let validation_err = validation_err.unwrap();
                        let err_msg = &validation_err[0].message;
                        let em = err_msg.clone();
                        let em = em.expect("failed to unwrap error msg");
                        return Error {
                            code: ErrorCode::ServerError(1),
                            message: "validation error".into(),
                            data: Some(format!("{}", em).into()),
                        };
                    }
                }
                Error {
                    code: ErrorCode::ServerError(1),
                    message: "validation error".into(),
                    data: Some(format!("{:?}", e).into()),
                }
            }
            WriteError::MissingParams { e } => Error {
                code: ErrorCode::ServerError(-32602),
                message: "invalid params".into(),
                data: Some(format!("{}", e.message).into()),
            },
            err => Error {
                code: ErrorCode::InternalError,
                message: "internal error".into(),
                data: Some(format!("{:?}", err).into()),
            },
        }
    }
}

pub fn run() -> Result<(), Box<dyn StdError>> {
    info!("Starting up.");

    debug!("Creating interface for I2C device.");
    let i2c = I2cdev::new("/dev/i2c-1")?;

    let mut disp: GraphicsMode<_> = Builder::new().connect_i2c(i2c).into();

    info!("Initializing the display.");
    disp.init().unwrap();
    disp.flush().unwrap();

    let oled = Arc::new(Mutex::new(disp));
    let oled_clone = Arc::clone(&oled);
    
    info!("Creating JSON-RPC I/O handler.");
    let mut io = IoHandler::default();

    io.add_method("write", move |params: Params| {
        info!("Received a 'write' request.");
        // parse parameters and match on result
        let m: Result<Msg, Error> = params.parse();
        match m {
            // if result contains parameters, unwrap
            Ok(_) => {
                let m: Msg = m.unwrap();
                match m.validate() {
                    Ok(_) => {
                        let mut oled = oled_clone.lock().unwrap();
                        if m.font_size == "6x8".to_string() {
                            oled.draw(
                                Font6x8::render_str(&format!("{}", &m.string))
                                    .translate(Coord::new(m.x_coord, m.y_coord))
                                    .into_iter(),
                            );
                        } else if m.font_size == "6x12".to_string() {
                            oled.draw(
                                Font6x12::render_str(&format!("{}", &m.string))
                                    .translate(Coord::new(m.x_coord, m.y_coord))
                                    .into_iter(),
                            );
                        } else if m.font_size == "8x16".to_string() {
                            oled.draw(
                                Font8x16::render_str(&format!("{}", &m.string))
                                    .translate(Coord::new(m.x_coord, m.y_coord))
                                    .into_iter(),
                            );
                        } else if m.font_size == "12x16".to_string() {
                            oled.draw(
                                Font12x16::render_str(&format!("{}", &m.string))
                                    .translate(Coord::new(m.x_coord, m.y_coord))
                                    .into_iter(),
                            );
                        }
                        oled.flush().unwrap();
                        Ok(Value::String("success".into()))
                    }
                    Err(e) => Err(Error::from(WriteError::Invalid { e })),
                }
            }
            Err(e) => Err(Error::from(WriteError::MissingParams { e })),
        }
    });

    let oled_clone = Arc::clone(&oled);

    io.add_method("clear", move |_| {
        let mut oled = oled_clone.lock().unwrap();
        let _ = oled.clear();
        oled.flush().unwrap();
        info!("Cleared the display.");
        Ok(Value::String("success".into()))
    });

    info!("Creating JSON-RPC server.");
    let server = ServerBuilder::new(io)
        .cors(DomainsValidation::AllowOnly(vec![
            AccessControlAllowOrigin::Null,
        ]))
        .start_http(&"127.0.0.1:3031".parse().unwrap())
        .expect("Unable to start RPC server");

    info!("Listening for requests.");
    server.wait();

    Ok(())
}
