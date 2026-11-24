// Copyright (C) 2026 DiamaneOS
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! DiamaneOS debug-only failure before creating a decoder child, without another property policy.

use std::ffi::OsStr;

pub fn before_child<'a>(
    name: &str,
    args: impl Iterator<Item = &'a OsStr>,
    debuggable: bool,
) -> bool {
    debuggable
        && name == "decoder"
        && args.filter_map(OsStr::to_str).any(|arg| {
            arg.split_ascii_whitespace().any(|word| word == "diamaneos.debug.fail_spawn")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn marker() -> impl Iterator<Item = &'static OsStr> {
        [OsStr::new("--params"), OsStr::new("console=ttyS0 diamaneos.debug.fail_spawn")].into_iter()
    }
    #[test]
    fn a_user_build_ignores_the_marker() {
        assert!(!before_child("decoder", marker(), false));
    }
    #[test]
    fn only_the_decoder_name_can_force_the_failure() {
        assert!(!before_child("filesystem", marker(), true));
        assert!(before_child("decoder", marker(), true));
    }
    #[test]
    fn the_marker_is_an_exact_word() {
        assert!(!before_child(
            "decoder",
            [OsStr::new("diamaneos.debug.fail_spawn_extra")].into_iter(),
            true
        ));
    }
}
