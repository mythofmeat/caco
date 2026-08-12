FROM archlinux:base-devel AS build

ARG VERSION
ARG PACKAGE_VERSION

RUN pacman -Syu --noconfirm rust clang git openssl pkgconf hicolor-icon-theme desktop-file-utils
RUN useradd --create-home builder
RUN install -d -o builder -g builder /src

WORKDIR /src
COPY --chown=builder:builder . .

RUN test -n "$VERSION" && \
    test -n "$PACKAGE_VERSION" && \
    sed -i "s/^version = \"0.0.0\"/version = \"$VERSION\"/" Cargo.toml && \
    sed -i "s/^pkgver=.*/pkgver=$PACKAGE_VERSION/" contrib/arch/PKGBUILD

USER builder
WORKDIR /src/contrib/arch
RUN makepkg --noconfirm

FROM scratch AS artifacts
COPY --from=build /src/contrib/arch/*.pkg.tar.zst /
