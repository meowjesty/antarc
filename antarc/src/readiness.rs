use std::time::Duration;

use log::debug;

use crate::net::{NetManager, NetworkResource};

#[derive(Debug)]
pub(crate) struct Readable;

#[derive(Debug)]
pub(crate) struct Writable;

impl<T> NetManager<T> {
    pub(crate) fn check_readiness(&mut self) {
        let world = &mut self.world;
        let mut readable = None;
        let mut writable = None;

        if let Some((_, resource)) = world.query::<&mut NetworkResource>().iter().next() {
            let _ = resource
                .poll
                .poll(&mut resource.events, Some(Duration::from_millis(100)))
                .unwrap();

            for event in resource.events.iter() {
                match event.token() {
                    NetworkResource::TOKEN => {
                        if event.is_readable() {
                            readable = Some(Readable);
                            debug!(
                                "{} {}:{} -> socket is readable",
                                file!(),
                                line!(),
                                column!()
                            );
                        } else if event.is_writable() {
                            writable = Some(Writable);
                            debug!(
                                "{} {}:{} -> socket is writable",
                                file!(),
                                line!(),
                                column!()
                            );
                        } else {
                            panic!("Unhandled network event readiness {:#?}.", event);
                        }
                    }
                    _ => {
                        panic!("Invalid token {:#?}.", event);
                    }
                }
            }
        }

        if let Some(readable) = readable {
            let readable_id = world.spawn((readable,));
        } else if let Some(writable) = writable {
            let writable_id = world.spawn((writable,));
        }
    }
}
