/* Minimal guest transport probe. It accepts one AF_VSOCK connection on the
 * fixed service port and echoes bounded bytes; it does not expose a shell. */
#include <errno.h>
#include <linux/vm_sockets.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
    const unsigned int port = 7000;
    int listener = socket(AF_VSOCK, SOCK_STREAM, 0);
    if (listener < 0) { perror("vsock socket"); return 1; }
    struct sockaddr_vm address = { .svm_family = AF_VSOCK, .svm_cid = VMADDR_CID_ANY, .svm_port = port };
    if (bind(listener, (struct sockaddr *)&address, sizeof(address)) < 0 || listen(listener, 1) < 0) { perror("vsock bind/listen"); return 1; }
    puts("ALCATRAZ_VSOCK_LISTENING port=7000"); fflush(stdout);
    int peer = accept(listener, NULL, NULL);
    if (peer < 0) { perror("vsock accept"); return 1; }
    char buffer[4096]; ssize_t count = read(peer, buffer, sizeof(buffer));
    if (count > 0) { const char prefix[] = "ALCATRAZ_VSOCK_ECHO "; if (write(peer, prefix, sizeof(prefix) - 1) < 0 || write(peer, buffer, (size_t)count) < 0) return 1; }
    close(peer); close(listener); return 0;
}
