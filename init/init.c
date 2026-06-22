typedef unsigned long usize;
typedef long i64;

#define NULL ((void*)0)

static long syscall(long nr, long a0, long a1, long a2)
{
    register long r0 asm("a0") = a0;
    register long r1 asm("a1") = a1;
    register long r2 asm("a2") = a2;
    register long r7 asm("a7") = nr;
    asm volatile("ecall" : "+r"(r0) : "r"(r1), "r"(r2), "r"(r7) : "memory");
    return r0;
}

static void puts(const char *s)
{
    long len = 0;
    while (s[len]) len++;
    syscall(1, 1, (long)s, len);  /* SYS_write = 1 */
}

static int readline(char *buf, int max)
{
    long n = syscall(2, 0, (long)buf, max);  /* SYS_read = 2 */
    buf[n] = 0;
    /* strip trailing newline */
    int len = (int)n;
    while (len > 0 && (buf[len-1] == '\n' || buf[len-1] == '\r')) buf[--len] = 0;
    return len;
}

static int strcmp(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return *(const unsigned char*)a - *(const unsigned char*)b;
}

static const char *skip(const char *s)
{
    while (*s == ' ' || *s == '\t') s++;
    return s;
}

static int starts_with(const char *s, const char *prefix)
{
    while (*prefix) {
        if (*s++ != *prefix++) return 0;
    }
    return 1;
}

__attribute__((section(".text.start")))
void _start(void)
{
    syscall(1, 1, (long)"\n  SlipperOS Shell v0.1\n  Type 'help' for commands\n\n", 52);

    char line[256];
    for (;;) {
        syscall(1, 1, (long)"SlipperOS# ", 11);
        int n = readline(line, 256);
        if (n == 0) continue;
        const char *cmd = line;

        if (strcmp(cmd, "help") == 0) {
            puts("\n  help       - this help\n");
            puts("  echo <t>   - print text\n");
            puts("  cat <f>    - print file\n");
            puts("  exec <f>   - run SPX program\n");
            puts("  clear      - clear screen\n");
            puts("  exit       - halt\n");
            continue;
        }
        if (starts_with(cmd, "echo ")) {
            puts(skip(cmd + 5));
            puts("\n");
            continue;
        }
        if (starts_with(cmd, "cat ")) {
            const char *path = skip(cmd + 4);
            int fd = (int)syscall(8, (long)path, 0, 0);
            if (fd < 0) { puts("cat: cannot open\n"); continue; }
            char buf[256];
            long r;
            while ((r = syscall(2, fd, (long)buf, 256)) > 0) {
                syscall(1, 1, (long)buf, r);
            }
            syscall(9, fd, 0, 0);
            continue;
        }
        if (starts_with(cmd, "exec ")) {
            const char *path = skip(cmd + 5);
            long r = syscall(12, (long)path, 0, 0);
            if (r < 0) { puts("exec: failed\n"); continue; }
            continue;
        }
        if (strcmp(cmd, "clear") == 0) {
            syscall(1, 1, (long)"\033[2J\033[H", 7);
            continue;
        }
        if (strcmp(cmd, "exit") == 0) {
            syscall(3, 0, 0, 0);  /* SYS_exit = 3 */
        }
        puts("? ");
        puts(line);
        puts("\n");
    }
}
