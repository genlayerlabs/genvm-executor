#include "platform.h"

#include "softfloat.h"

// Berkeley leaves this to the platform. Recording the flags is what lets the
// trapping truncations tell a wasm-undefined input from an ordinary result.
void
softfloat_raiseFlags(uint_fast8_t flags)
{
	softfloat_exceptionFlags |= flags;
}
