/* ST04 fixed diagnostic only. Closed probes never delegate arbitrary code.
 * Controller owns readiness/release and independently inventories the probe path.
 */
#include <errno.h>
#include <limits.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <signal.h>
#include <spawn.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <netinet/in.h>
#include <sys/stat.h>
#include <unistd.h>

static int emit(int fd, const char *bytes, size_t size) {
    while (size) {
        ssize_t n = write(fd, bytes, size);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) return 1;
        bytes += n;
        size -= (size_t)n;
    }
    return 0;
}
static int denied(int result, int error) {
    return result == -1 && (error == EPERM || error == EACCES);
}
static int process_probes(const char *self) {
    pid_t child = fork();
    if (child == 0) _exit(0);
    if (child > 0) {
        int status = 0;
        if (waitpid(child, &status, 0) != child || status != 0) return 1;
        char evidence[96];
        int length = snprintf(evidence, sizeof(evidence), "ST04-CHILD fork %ld wait %d\n", (long)child, status);
        if (length < 0 || (size_t)length >= sizeof(evidence) || emit(1, evidence, (size_t)length)) return 1;
        if (emit(1, "ST04-FORK-ALLOWED\n", sizeof("ST04-FORK-ALLOWED\n") - 1)) return 1;
    } else {
        if (!denied(-1, errno)) return 1;
        if (emit(1, "ST04-FORK-DENIED\n", sizeof("ST04-FORK-DENIED\n") - 1)) return 1;
    }
    char *args[] = {(char *)self, "--child", NULL};
    char *env[] = {NULL};
    child = -1;
    int error = posix_spawn(&child, self, NULL, NULL, args, env);
    if (error == 0) {
        int status = 0;
        if (waitpid(child, &status, 0) != child || status != 0) return 1;
        char evidence[96];
        int length = snprintf(evidence, sizeof(evidence), "ST04-CHILD spawn %ld wait %d\n", (long)child, status);
        if (length < 0 || (size_t)length >= sizeof(evidence) || emit(1, evidence, (size_t)length)) return 1;
        return emit(1, "ST04-SPAWN-ALLOWED\n", sizeof("ST04-SPAWN-ALLOWED\n") - 1);
    }
    if (error != EPERM && error != EACCES) return 1;
    return emit(1, "ST04-SPAWN-DENIED\n", sizeof("ST04-SPAWN-DENIED\n") - 1);
}
static int network_probe(const char *port_text) {
    char *end = NULL;
    long port = strtol(port_text, &end, 10);
    if (!end || *end || port < 1 || port > 65535) return -1;
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return denied(-1, errno) ? 0 : -1;
    struct sockaddr_in address;
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons((unsigned short)port);
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    int result = connect(fd, (struct sockaddr *)&address, sizeof(address));
    int error = errno;
    if (result == 0 && emit(fd, "ST04-NETWORK\n", sizeof("ST04-NETWORK\n") - 1)) { close(fd); return -1; }
    close(fd);
    return result == 0 ? 1 : (denied(result, error) ? 0 : -1);
}
int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--child")) _exit(0);
    if (argc != 6) return 64;
    size_t parent_length = strlen(argv[4]);
    if (parent_length == 0 || parent_length > 10) return 78;
    for (size_t i = 0; i < parent_length; ++i)
        if (argv[4][i] < '0' || argv[4][i] > '9') return 78;
    char *parent_end = NULL;
    errno = 0;
    long parent_number = strtol(argv[4], &parent_end, 10);
    if (errno || !parent_end || parent_end == argv[4] || *parent_end
        || parent_number <= 1 || parent_number > INT_MAX) return 78;
    pid_t expected_parent = (pid_t)parent_number;
    if (getppid() != expected_parent) return 78;
    if (strcmp(argv[5], "check") && strcmp(argv[5], "signal")) return 79;
    /* Only inherited stdio may survive the audited native spawn allowlist. */
    for (int fd = 3; fd < 256; ++fd) {
        errno = 0;
        if (fcntl(fd, F_GETFD) != -1 || errno != EBADF) return 65;
    }
    if (emit(1, "ST04-READY\n", sizeof("ST04-READY\n") - 1)) return 66;
    struct pollfd input = { .fd = 0, .events = POLLIN };
    if (poll(&input, 1, 4000) != 1) return 67;
    char release = 0;
    if (read(0, &release, 1) != 1 || release != 'R') return 68;
    errno = 0;
    int fd = open(argv[1], O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0600);
    int saved = errno;
    if (fd >= 0) {
        /* Positive control has exactly one bounded, independently observed effect. */
        int failed = emit(fd, "ST04-EFFECT\n", 12);
        if (close(fd) || failed) return 69;
        if (emit(1, "ST04-WRITE-ALLOWED\n", sizeof("ST04-WRITE-ALLOWED\n") - 1)) return 70;
    } else if (saved == EPERM || saved == EACCES) {
        if (emit(1, "ST04-WRITE-DENIED\n", sizeof("ST04-WRITE-DENIED\n") - 1)) return 71;
    } else return 72;
    errno = 0;
    fd = open(argv[2], O_RDONLY | O_NOFOLLOW);
    saved = errno;
    if (fd >= 0) {
        char witness[19];
        ssize_t count = read(fd, witness, sizeof(witness));
        close(fd);
        if (count != 18 || memcmp(witness, "ST04-READ-WITNESS\n", 18)) return 73;
    }
    if (fd >= 0) { if (emit(1, "ST04-READ-ALLOWED\n", sizeof("ST04-READ-ALLOWED\n") - 1)) return 73; }
    else { if (!denied(fd, saved) || emit(1, "ST04-READ-DENIED\n", sizeof("ST04-READ-DENIED\n") - 1)) return 73; }
    if (process_probes(argv[0])) return 74;
    int network = network_probe(argv[3]);
    if (network < 0) return 75;
    if (network == 1) {
        if (emit(1, "ST04-NETWORK-ALLOWED\n", sizeof("ST04-NETWORK-ALLOWED\n") - 1)) return 75;
    } else if (emit(1, "ST04-NETWORK-DENIED\n", sizeof("ST04-NETWORK-DENIED\n") - 1)) return 75;
    errno = 0;
    if (getppid() != expected_parent) return 78;
    int signal_result = kill(expected_parent, !strcmp(argv[5], "signal") ? SIGCONT : 0);
    if (signal_result == 0) {
        if (emit(1, "ST04-SIGNAL-ALLOWED\n", sizeof("ST04-SIGNAL-ALLOWED\n") - 1)) return 76;
    } else if (!denied(signal_result, errno) || emit(1, "ST04-SIGNAL-DENIED\n", sizeof("ST04-SIGNAL-DENIED\n") - 1)) return 76;
    return emit(2, "ST04-STDERR\n", sizeof("ST04-STDERR\n") - 1);
}
