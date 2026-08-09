// SPDX-License-Identifier: GPL-2.0-or-later
#include "protocol.hpp"

#include <csignal>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>

#include "led-matrix.h"

namespace {
volatile std::sig_atomic_t g_stop = 0;

void handle_signal(int) { g_stop = 1; }

int env_int(const char* name, int fallback, int minimum, int maximum) {
  const char* raw = std::getenv(name);
  if (raw == nullptr || *raw == '\0') return fallback;
  std::size_t parsed = 0;
  const int value = std::stoi(raw, &parsed);
  if (raw[parsed] != '\0' || value < minimum || value > maximum) {
    throw std::runtime_error(std::string("invalid ") + name);
  }
  return value;
}

struct MatrixDeleter {
  void operator()(rgb_matrix::RGBMatrix* matrix) const {
    if (matrix != nullptr) {
      matrix->Clear();
      delete matrix;
    }
  }
};

void paint_packet(
    rgb_matrix::FrameCanvas* canvas,
    const leddy::hub75::FramePacket& packet,
    int physical_width,
    int physical_height) {
  canvas->Clear();
  for (std::uint16_t y = 0; y < packet.height; ++y) {
    for (std::uint16_t x = 0; x < packet.width; ++x) {
      const std::size_t index = static_cast<std::size_t>(y) * packet.width + x;
      const std::uint8_t intensity =
          leddy::hub75::scale_brightness(packet.pixels[index], packet.brightness);
      if (intensity == 0U) continue;
      const auto point =
          leddy::hub75::map_logical_pixel(x, y, packet, physical_width, physical_height);
      canvas->SetPixel(point.x, point.y, intensity, intensity, intensity);
    }
  }
}
}  // namespace

int main() {
  try {
    std::signal(SIGINT, handle_signal);
    std::signal(SIGTERM, handle_signal);

    const int rows = env_int("LEDDY_HUB75_ROWS", 32, 1, 128);
    const int cols = env_int("LEDDY_HUB75_COLS", 64, 1, 256);
    const int chain = env_int("LEDDY_HUB75_CHAIN", 5, 1, 32);
    const int parallel = env_int("LEDDY_HUB75_PARALLEL", 1, 1, 3);

    rgb_matrix::RGBMatrix::Options options;
    options.rows = rows;
    options.cols = cols;
    options.chain_length = chain;
    options.parallel = parallel;
    options.hardware_mapping = "regular";

    rgb_matrix::RuntimeOptions runtime;
    std::unique_ptr<rgb_matrix::RGBMatrix, MatrixDeleter> matrix(
        rgb_matrix::RGBMatrix::CreateFromOptions(options, runtime));
    if (!matrix) throw std::runtime_error("failed to create HUB75 RGB matrix");

    const int physical_width = cols * chain;
    const int physical_height = rows * parallel;
    if (physical_width < leddy::hub75::kMaxLogicalWidth ||
        physical_height < leddy::hub75::kMaxLogicalHeight) {
      throw std::runtime_error("configured HUB75 canvas cannot fit the maximum Leddy viewport");
    }

    rgb_matrix::FrameCanvas* canvas = matrix->CreateFrameCanvas();
    canvas->Clear();
    canvas = matrix->SwapOnVSync(canvas);

    std::cerr << "leddy-hub75-helper online physical=" << physical_width << "x"
              << physical_height << " rows=" << rows << " cols=" << cols
              << " chain=" << chain << " parallel=" << parallel << '\n';

    leddy::hub75::FramePacket packet;
    while (!g_stop && leddy::hub75::read_packet(std::cin, packet)) {
      paint_packet(canvas, packet, physical_width, physical_height);
      canvas = matrix->SwapOnVSync(canvas);
    }

    canvas->Clear();
    matrix->SwapOnVSync(canvas);
    matrix->Clear();
    return 0;
  } catch (const std::exception& error) {
    std::cerr << "leddy-hub75-helper: " << error.what() << '\n';
    return 1;
  }
}
