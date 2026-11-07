// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn main() {
    let mut buffer = itoa::Buffer::new();
    println!("velnor-native-image-qualification:{}", buffer.format(547));
}
