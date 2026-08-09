// SPDX-License-Identifier: GPL-2.0-or-later
#include "protocol.hpp"

#include <cassert>
#include <cstdint>
#include <sstream>
#include <string>

namespace {
std::string packet_bytes(
    std::uint8_t origin,
    bool serpentine,
    std::uint8_t brightness,
    std::uint16_t width,
    std::uint16_t height,
    const std::string& payload) {
  std::string bytes(leddy::hub75::kMagic.begin(), leddy::hub75::kMagic.end());
  bytes.push_back(static_cast<char>(leddy::hub75::kVersion));
  bytes.push_back(static_cast<char>(origin));
  bytes.push_back(static_cast<char>(serpentine ? 1 : 0));
  bytes.push_back(static_cast<char>(brightness));
  bytes.push_back(static_cast<char>(width & 0xffU));
  bytes.push_back(static_cast<char>((width >> 8U) & 0xffU));
  bytes.push_back(static_cast<char>(height & 0xffU));
  bytes.push_back(static_cast<char>((height >> 8U) & 0xffU));
  const std::uint32_t length = static_cast<std::uint32_t>(payload.size());
  for (unsigned shift : {0U, 8U, 16U, 24U}) {
    bytes.push_back(static_cast<char>((length >> shift) & 0xffU));
  }
  bytes += payload;
  return bytes;
}

void test_parse_packet() {
  std::istringstream input(packet_bytes(3, true, 96, 3, 2, "\x01\x02\x03\x04\x05\x06"));
  leddy::hub75::FramePacket packet;
  assert(leddy::hub75::read_packet(input, packet));
  assert(packet.origin == 3);
  assert(packet.serpentine);
  assert(packet.brightness == 96);
  assert(packet.width == 3);
  assert(packet.height == 2);
  assert(packet.pixels.size() == 6);
  assert(packet.pixels[0] == 1);
  assert(packet.pixels[5] == 6);
  assert(!leddy::hub75::read_packet(input, packet));
}

void test_centered_top_left_mapping() {
  leddy::hub75::FramePacket packet;
  packet.origin = 0;
  packet.serpentine = false;
  packet.width = 300;
  packet.height = 20;
  const auto top_left = leddy::hub75::map_logical_pixel(0, 0, packet, 320, 32);
  const auto bottom_right = leddy::hub75::map_logical_pixel(299, 19, packet, 320, 32);
  assert(top_left.x == 10 && top_left.y == 6);
  assert(bottom_right.x == 309 && bottom_right.y == 25);
}

void test_serpentine_bottom_right_mapping() {
  leddy::hub75::FramePacket packet;
  packet.origin = 3;
  packet.serpentine = true;
  packet.width = 4;
  packet.height = 2;

  const auto logical_top_left = leddy::hub75::map_logical_pixel(0, 0, packet, 8, 4);
  const auto logical_bottom_left = leddy::hub75::map_logical_pixel(0, 1, packet, 8, 4);
  assert(logical_top_left.x == 2 && logical_top_left.y == 2);
  assert(logical_bottom_left.x == 5 && logical_bottom_left.y == 1);
}

void test_brightness_scaling() {
  assert(leddy::hub75::scale_brightness(0, 96) == 0);
  assert(leddy::hub75::scale_brightness(255, 96) == 96);
  assert(leddy::hub75::scale_brightness(128, 128) == 64);
}
}  // namespace

int main() {
  test_parse_packet();
  test_centered_top_left_mapping();
  test_serpentine_bottom_right_mapping();
  test_brightness_scaling();
  return 0;
}
