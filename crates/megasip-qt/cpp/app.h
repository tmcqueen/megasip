#pragma once
#include <cstdint>
#include "rust/cxx.h"

int32_t run_qt_app(rust::Str app_id, rust::Str icon_path, bool start_hidden);
// Switch this process's app_id and window icon (an empty path: MegaSIP's own icon).
void set_app_identity(rust::Str app_id, rust::Str icon_path);
