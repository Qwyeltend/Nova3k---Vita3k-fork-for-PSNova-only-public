#!/usr/bin/env python3
"""Nova3K: talks to the PSO2 server like the PS Vita client does, up to the login packet.

Ship list (12094) -> block balance (12193) -> block server: hello 03-08, key exchange 11-0B (RSA with the
built-in server key from vita3k/nova3k/pso2_key.h) -> 11-0C, then RC4. Prints what the server answers.
Usage: pso2_smoke_client.py <path to pso2_key.h>
"""
import os, re, socket, struct, sys, time
from cryptography.hazmat.primitives.asymmetric import rsa, padding

src = open(sys.argv[1]).read()
m = re.search(r'modulus\{ \{(.*?)\} \}', src, re.S)
n = int.from_bytes(bytes(int(x, 16) for x in re.findall(r'0x([0-9A-Fa-f]{2})', m.group(1))), 'big')

def rc4(key):
    S = list(range(256)); j = 0
    for i in range(256):
        j = (j + S[i] + key[i % len(key)]) & 255; S[i], S[j] = S[j], S[i]
    st = [0, 0]
    def f(data):
        i, j = st; out = bytearray()
        for b in data:
            i = (i + 1) & 255; j = (j + S[i]) & 255; S[i], S[j] = S[j], S[i]
            out.append(b ^ S[(S[i] + S[j]) & 255])
        st[0], st[1] = i, j
        return bytes(out)
    return f

def packets(data):
    out = []
    while len(data) >= 8:
        size, t, s = struct.unpack_from('<IBB', data)
        if size < 8: break
        out.append((t, s, data[:size])); data = data[size:]
    return out

def show(name, data):
    print(f'{name}: {len(data)} bytes')
    for t, s, p in packets(data):
        print(f'  {t:02X}-{s:02X} ({len(p)} bytes): {p[:64].hex(" ")}')

def recv_until_close(c, timeout=5):
    c.settimeout(timeout); data = b''
    try:
        while True:
            chunk = c.recv(65536)
            if not chunk: break
            data += chunk
    except socket.timeout:
        pass
    return data

ok = True
c = socket.create_connection(('127.0.0.1', 12094)); show('ship list 12094', recv_until_close(c))
# block balance; PhantasyServer answers with its ship's own balance port first, then the block server
port, hops = 12193, 0
while True:
    c = socket.create_connection(('127.0.0.1', port)); c.settimeout(5)
    first = c.recv(4096)
    pk = packets(first)
    if not pk or (pk[0][0], pk[0][1]) != (0x11, 0x2C):
        break
    show(f'block balance {port}', first)
    body = pk[0][2]
    ip, port = body[0x68:0x6C], struct.unpack_from('<H', body, 0x6C)[0]
    print(f'  -> go to {".".join(map(str, ip))}:{port}')
    c.close(); hops += 1
    if hops > 3:
        print('FAIL: block balance loop'); sys.exit(1)
if hops == 0:
    print('FAIL: no 11-2C block balance answer'); sys.exit(1)
print(f'block server: port {port}')
hello = first; show('hello', hello)
token, key = os.urandom(16), os.urandom(16)
ct = rsa.RSAPublicNumbers(65537, n).public_key().encrypt(token + key, padding.PKCS1v15())
c.send(struct.pack('<IBBBB', 8 + 0x80 + 4, 0x11, 0x0B, 0, 0) + ct[::-1] + b'\0' * 4)
dec, enc = rc4(key), rc4(key)
ans = dec(c.recv(4096)); show('key exchange answer', ans)
if ans[8:24] != rc4(key)(token):
    print('FAIL: 11-0C does not contain the encrypted token'); sys.exit(1)
print('key exchange OK')
time.sleep(1)
