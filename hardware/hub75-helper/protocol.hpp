// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once

#include <array>
#include <cstdint>
#include <istream>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

namespace leddy::hub75 {

constexpr std::array<char, 8> kMagic{'L', 'E', 'D', 'D', 'Y', 'F', '0', '1'};
constexpr std::uint8_t kVersion = 1;
constexpr std::uint16_t kMaxLogicalWidth = 300;
constexpr std::uint16_t kMaxLogicalHeight = 20;

struct FramePacket {
  std::uint8_t origin = 0;
  bool serpentine = false;
  std::uint8_t brightness = 0;
  std::uint16_t width = 0;
  std::uint16_t height = 0;
  std::vector<std::uint8_t> pixels;
};

inline bool read_exact(std::istream& input, char* destination, std::size_t size) {
  input.read(destination, static_cast<std::streamsize>(size));
  if (input.gcount() == 0 && input.eof()) return false;
  if (input.gcount() != static_cast<std::streamsize>(size)) {
    throw std::runtime_error("truncated LEDDYF01 packet");
  }
  return true;
}

inline std::uint16_t little_u16(const std::array<std::uint8_t, 2>& bytes) {
  return static_cast<std::uint16_t>(bytes[0]) |
         static_cast<std::uint16_t>(bytes[1]) << 8U;
}

inline std::uint32_t little_u32(const std::array<std::uint8_t, 4>& bytes) {
  return static_cast<std::uint32_t>(bytes[0]) |
         static_cast<std::uint32_t>(bytes[1]) << 8U |
         static_cast<std::uint32_t>(bytes[2]) << 16U |
         static_cast<std::uint32_t>(bytes[3]) << 24U;
}

inline bool read_packet(std::istream& input, FramePacket& packet) {
  std::array<char, 8> magic{};
  if (!read_exact(input, magic.data(), magic.size())) return false;
  if (magic != kMagic) throw std::runtime_error("invalid LEDDYF01 packet magic");

  std::array<std::uint8_t, 4> metadata{};
  if (!read_exact(input, reinterpret_cast<char*>(metadata.data()), metadata.size())) {
    throw std::runtime_error("truncated LEDDYF01 metadata");
  }
  if (metadata[0] != kVersion) throw std::runtime_error("unsupported LEDDYF01 version");
  if (metadata[1] > 3U) throw std::runtime_error("invalid LEDDY pixel origin");
  if (metadata[2] > 1U) throw std::runtime_error("invalid LEDDY serpentine flag");

  std::array<std::uint8_t, 2> width_bytes{};
  std::array<std::uint8_t, 2> height_bytes{};
  std::array<std::uint8_t, 4> length_bytes{};
  if (!read_exact(input, reinterpret_cast<char*>(width_bytes.data()), width_bytes.size()) ||
      !read_exact(input, reinterpret_cast<char*>(height_bytes.data()), height_bytes.size()) ||
      !read_exact(input, reinterpret_cast<char*>(length_bytes.data()), length_bytes.size())) {
    throw std::runtime_error("truncated LEDDYF01 geometry");
  }

  const std::uint16_t width = little_u16(width_bytes);
  const std::uint16_t height = little_u16(height_bytes);
  const std::uint32_t payload_length = little_u32(length_bytes);
  if (width == 0U || width > kMaxLogicalWidth || height == 0U || height > kMaxLogicalHeight) {
    throw std::runtime_error("LEDDY logical geometry is outside the supported range");
  }
  const std::uint32_t expected = static_cast<std::uint32_t>(width) * height;
  if (payload_length != expected) throw std::runtime_error("LEDDYF01 payload length mismatch");
  if (payload_length > static_cast<std::uint32_t>(std::numeric_limits<std::streamsize>::max())) {
    throw std::runtime_error("LEDDYF01 payload is too large");
  }

  std::vector<std::uint8_t> pixels(payload_length);
  if (!read_exact(input, reinterpret_cast<char*>(pixels.data()), pixels.size())) {
    throw std::runtime_error("truncated LEDDYF01 pixel payload");
  }

  packet.origin = metadata[1];
  packet.serpentine = metadata[2] != 0U;
  packet.brightness = metadata[3];
  packet.width = width;
  packet.height = height;
  packet.pixels = std::move(pixels);
  return true;
}

struct Point {
  int x;
  int y;
};

inline Point map_logical_pixel(
    std::uint16_t x,
    std::uint16_t y,
    const FramePacket& packet,
    int physical_width,
    int physical_height) {
  if (physical_width < packet.width || physical_height < packet.height) {
    throw std::runtime_error("physical HUB75 canvas is smaller than logical Leddy viewport");
  }

  const bool starts_right = packet.origin == 1U || packet.origin == 3U;
  const bool starts_bottom = packet.origin == 2U || packet.origin == 3U;
  const std::uint16_t physical_row = starts_bottom
      ? static_cast<std::uint16_t>(packet.height - 1U - y)
      : y;
  const bool row_starts_right = starts_right ^ (packet.serpentine && (physical_row % 2U == 1U));
  const std::uint16_t physical_column = row_starts_right
      ? static_cast<std::uint16_t>(packet.width - 1U - x)
      : x;

  const int x_offset = (physical_width - packet.width) / 2;
  const int y_offset = (physical_height - packet.height) / 2;
  return Point{x_offset + physical_column, y_offset + physical_row};
}

inline std::uint8_t scale_brightness(std::uint8_t pixel, std::uint8_t brightness) {
  return static_cast<std::uint8_t>(
      (static_cast<std::uint16_t>(pixel) * static_cast<std::uint16_t>(brightness) + 127U) / 255U);
}

}  // namespace leddy::hub75
