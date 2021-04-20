use std::time::Duration;

use hecs::World;

use crate::net::NetworkResource;

#[derive(Debug)]
pub(crate) struct Readable;

#[derive(Debug)]
pub(crate) struct Writable;

pub(crate) fn system_readiness(world: &mut World) {
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
                    } else if event.is_writable() {
                        writable = Some(Writable);
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
        let _ = world.spawn((readable,));
    } else if let Some(writable) = writable {
        let _ = world.spawn((writable,));
    }
}
