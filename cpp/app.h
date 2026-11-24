#pragma once
#include <cstdint>
#include "rust/cxx.h"

int32_t run_qt_app(rust::Str app_id, bool start_hidden);
