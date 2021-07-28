use std::time::Duration;

use log::debug;

use crate::net::{NetManager, NetworkResource};

#[derive(Debug)]
pub struct Readable;

#[derive(Debug)]
pub struct Writable;

impl<T> NetManager<T> {
    pub fn check_readiness(&mut self) {
        let world = &mut self.world;
        let mut readable = None;
        let mut writable = None;

        for (resource_id, resource) in world.query_mut::<&mut NetworkResource>() {
            debug!(
                "{} {}:{} -> checking resource readiness {:#?}",
                file!(),
                line!(),
                column!(),
                resource
            );

            let _ = resource
                .poll
                .poll(&mut resource.events, Some(Duration::from_millis(100)))
                .unwrap();

            // TODO(alex) 2021-05-09: Figure out a way to do this async, I don't like the blocking
            // and it's not working properly anyway.
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
                        }
                        if event.is_writable() {
                            writable = Some(Writable);
                            debug!(
                                "{} {}:{} -> socket is writable",
                                file!(),
                                line!(),
                                column!()
                            );
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }

        if let Some(readable) = readable {
            let readable_id = world.spawn((readable,));
        }
        if let Some(writable) = writable {
            let writable_id = world.spawn((writable,));
        }
    }
}
