package io.flowcatalyst.sdk;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.function.Function;

/**
 * Minimal HTTP stub for SDK tests: register handlers per method+path,
 * records every request (method, path+query, auth header, body).
 */
public final class StubServer implements AutoCloseable {

    public record Recorded(String method, String pathAndQuery, String authorization, String body) {}

    public record Reply(int status, String body) {
        public static Reply json(int status, String body) {
            return new Reply(status, body);
        }
    }

    private record Route(String method, String path, Function<Recorded, Reply> handler) {}

    private final HttpServer server;
    private final List<Route> routes = new ArrayList<>();
    public final List<Recorded> requests = new CopyOnWriteArrayList<>();

    public StubServer() throws IOException {
        server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", this::dispatch);
        server.start();
    }

    public String baseUrl() {
        return "http://127.0.0.1:" + server.getAddress().getPort();
    }

    /** Register a handler; first match on (method, exact path) wins. */
    public void on(String method, String path, Function<Recorded, Reply> handler) {
        routes.add(new Route(method, path, handler));
    }

    public void on(String method, String path, int status, String body) {
        on(method, path, r -> new Reply(status, body));
    }

    /** Standard token endpoint returning the given access token. */
    public void stubToken(String token) {
        on("POST", "/oauth/token", 200,
                "{\"access_token\":\"" + token + "\",\"token_type\":\"Bearer\",\"expires_in\":3600}");
    }

    private void dispatch(HttpExchange exchange) throws IOException {
        String body = new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
        String query = exchange.getRequestURI().getRawQuery();
        Recorded recorded = new Recorded(
                exchange.getRequestMethod(),
                exchange.getRequestURI().getPath() + (query != null ? "?" + query : ""),
                exchange.getRequestHeaders().getFirst("Authorization"),
                body);
        requests.add(recorded);

        Reply reply = null;
        for (Route route : routes) {
            if (route.method().equals(recorded.method())
                    && route.path().equals(exchange.getRequestURI().getPath())) {
                reply = route.handler().apply(recorded);
                break;
            }
        }
        if (reply == null) {
            reply = new Reply(404, "{\"error\":\"NOT_FOUND\",\"message\":\"no stub for "
                    + recorded.method() + " " + exchange.getRequestURI().getPath() + "\"}");
        }

        byte[] payload = reply.body() == null ? new byte[0] : reply.body().getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", "application/json");
        exchange.sendResponseHeaders(reply.status(), reply.status() == 204 ? -1 : payload.length);
        if (reply.status() != 204) {
            try (OutputStream out = exchange.getResponseBody()) {
                out.write(payload);
            }
        }
        exchange.close();
    }

    @Override
    public void close() {
        server.stop(0);
    }
}
