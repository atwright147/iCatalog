# This file consumes the minimal context prepared by build.sh, not the repository.
FROM scratch
ARG VERSION
LABEL org.opencontainers.image.title="iCatalog" \
      org.opencontainers.image.description="iCatalog NAS catalogue sync service" \
      org.opencontainers.image.source="https://github.com/atwright147/iCatalog" \
      org.opencontainers.image.version="$VERSION"
COPY --chmod=755 icatalog /icatalog
COPY --chown=1000:1000 catalogue/ /catalogue/
COPY --chown=1000:1000 media/ /media/
USER 1000:1000
WORKDIR /catalogue
EXPOSE 8080
ENTRYPOINT ["/icatalog"]
CMD ["serve", "/catalogue/catalogue.sqlite", "--bind", "0.0.0.0", "--port", "8080"]
