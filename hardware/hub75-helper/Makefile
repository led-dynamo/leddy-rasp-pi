CXX ?= g++
CXXFLAGS ?= -std=c++20 -O2 -Wall -Wextra -Werror -pedantic
RGB_MATRIX_DIR ?= /opt/rpi-rgb-led-matrix

.PHONY: all protocol-test test clean

all: leddy-hub75-helper

protocol-test: protocol_test.cc protocol.hpp
	$(CXX) $(CXXFLAGS) -I. protocol_test.cc -o protocol-test

# The physical target expects a separately installed checkout/build of
# hzeller/rpi-rgb-led-matrix. Its GPL licensing is intentionally not hidden
# behind the MIT Rust crate.
leddy-hub75-helper: main.cc protocol.hpp
	$(CXX) $(CXXFLAGS) -I. -I$(RGB_MATRIX_DIR)/include main.cc \
		-L$(RGB_MATRIX_DIR)/lib -lrgbmatrix -lrt -lm -lpthread -o $@

test: protocol-test
	./protocol-test

clean:
	rm -f protocol-test leddy-hub75-helper
