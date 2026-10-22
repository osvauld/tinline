use super::*;
use loro::VersionVector;
use std::sync::Arc;
use std::time::Duration;
use sync_runtime::Error;
use sync_runtime::frames::{FrameReader, FrameWriter, SelfMsg};
use group_runtime::own::contents;

mod wire_tests;
