#include <stdarg.h>
#include <stdio.h>

extern void gp_rust_progress(void *privdata, int level, const char *msg);

void gp_progress_trampoline(void *privdata, int level, const char *fmt, ...) {
	va_list ap;
	char buf[4096];

	va_start(ap, fmt);
	vsnprintf(buf, sizeof(buf), fmt, ap);
	va_end(ap);
	gp_rust_progress(privdata, level, buf);
}
