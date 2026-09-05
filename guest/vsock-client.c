/* Fixed-purpose Vibe-side client: connect only to host CID 2, port 7001. */
#include <errno.h>
#include <linux/vm_sockets.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
    int fd = socket(AF_VSOCK, SOCK_STREAM, 0);
    if (fd < 0) { perror("vsock socket"); return 1; }
    struct sockaddr_vm peer = { .svm_family = AF_VSOCK, .svm_cid = VMADDR_CID_HOST, .svm_port = 7001 };
    for (int i = 0; i < 50; ++i) {
        if (connect(fd, (struct sockaddr *)&peer, sizeof(peer)) == 0) break;
        if (i == 49) { perror("vsock connect"); return 1; }
        usleep(100000);
    }
    puts("ALCATRAZ_VSOCK_CLIENT_CONNECTED"); fflush(stdout);
    const char request[] = "vibe-to-codex";
    if (write(fd, request, sizeof(request) - 1) < 0) return 1;
    char response[256]; size_t total = 0; ssize_t count;
    while (total < sizeof(response) - 1 && (count = read(fd, response + total, sizeof(response) - 1 - total)) > 0) total += (size_t)count;
    if (total > 0) { response[total] = 0; printf("ALCATRAZ_VSOCK_CLIENT_RESPONSE %s\n", response); fflush(stdout); }
    close(fd);
    if (total == 0) return 1;
    /* Keep PID 1 alive until the supervisor intentionally tears down the VM. */
    for (;;) pause();
}
