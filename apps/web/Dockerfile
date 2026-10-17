# Tutor website (TanStack Start). Build context: repository root.
FROM node:22-slim AS base
RUN corepack enable
WORKDIR /app

FROM base AS prune
RUN npm i -g turbo@2.11.8
COPY . .
RUN turbo prune @tutor/web --docker

FROM base AS build
COPY --from=prune /app/out/json/ .
RUN pnpm install --frozen-lockfile
COPY --from=prune /app/out/full/ .
RUN pnpm turbo run build --filter=@tutor/web

FROM node:22-slim
WORKDIR /app
COPY --from=build /app/apps/web/.output ./.output
USER node
ENV PORT=3000 NODE_ENV=production
EXPOSE 3000
CMD ["node", ".output/server/index.mjs"]
