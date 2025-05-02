#!/bin/sh

if [ "$#" -ne 1 ]; then
  echo "Usage: Must supply a domain"
  exit 1
fi

DOMAIN=$1

openssl genrsa -des3 -out cacert.key 2048
openssl req -x509 -new -nodes -key cacert.key -sha256 -days 1825 -out cacert.pem
openssl genrsa -out "$DOMAIN".key 2048
openssl req -new -key "$DOMAIN".key -out "$DOMAIN".csr

cat >"$DOMAIN".ext <<EOF
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, nonRepudiation, keyEncipherment, dataEncipherment
subjectAltName = IP:$DOMAIN
EOF

openssl x509 -req -in "$DOMAIN".csr -CA cacert.pem -CAkey cacert.key -CAcreateserial -out "$DOMAIN".crt -days 825 -sha256 -extfile "$DOMAIN".ext
